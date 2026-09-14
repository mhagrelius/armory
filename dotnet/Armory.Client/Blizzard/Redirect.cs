using System.Net;
using System.Net.Sockets;
using System.Text;
using Armory.Blizzard;

namespace Armory.Client.Blizzard;

/// <summary>
/// The loopback listener the sign-in redirect lands on.
///
/// Battle.net rejects custom URI schemes and matches the registered redirect
/// exactly, with no loopback port wildcard, so a real HTTP server on a fixed
/// port on 127.0.0.1 is the only supported shape for a desktop client. It is
/// a raw socket rather than <c>HttpListener</c>: that one needs a URL
/// reservation on Windows for anything but an administrator, and a sign-in
/// that fails for want of a <c>netsh</c> command reads like a Blizzard
/// problem.
///
/// One shot. The browser will happily request <c>/favicon.ico</c> alongside
/// the callback, and a retried or refreshed redirect delivers the same code
/// twice; answering only once keeps the flow from being driven backwards.
/// Disposing unbinds the port, and the port is fixed and registered, so it
/// must not leak.
/// </summary>
public sealed class Redirect : IDisposable
{
    private readonly TcpListener listener;
    private readonly string expected;
    private readonly Action<Result<string, Reason>> deliver;
    private readonly CancellationTokenSource stopping = new();
    private bool spent;

    private Redirect(TcpListener listener, string expected, Action<Result<string, Reason>> deliver)
    {
        this.listener = listener;
        this.expected = expected;
        this.deliver = deliver;
    }

    /// <summary>The port this one is bound to. The registered one unless a test asked otherwise.</summary>
    public int Port { get; private init; }

    /// <summary>
    /// Start listening, and call <paramref name="deliver"/> when the browser
    /// comes back. <paramref name="state"/> is what was sent to the authorize
    /// endpoint; a redirect carrying anything else did not come from the flow
    /// we started and is refused. With no PKCE verifier to fall back on, this
    /// check is the only thing between the flow and a code somebody planted.
    /// </summary>
    public static Result<Redirect, string> Listen(string state, Action<Result<string, Reason>> deliver, int port = OAuth.RedirectPort)
    {
        var listener = new TcpListener(IPAddress.Loopback, port);
        try
        {
            listener.Start();
        }
        catch (SocketException error)
        {
            return Result<Redirect, string>.Err(error.Message);
        }
        var redirect = new Redirect(listener, state, deliver) { Port = ((IPEndPoint)listener.LocalEndpoint).Port };
        _ = redirect.Serve();
        return Result<Redirect, string>.Ok(redirect);
    }

    private async Task Serve()
    {
        while (!stopping.IsCancellationRequested)
        {
            TcpClient client;
            try
            {
                client = await listener.AcceptTcpClientAsync(stopping.Token);
            }
            catch (OperationCanceledException)
            {
                return;
            }
            catch (SocketException)
            {
                return;
            }
            catch (ObjectDisposedException)
            {
                return;
            }
            using (client)
            {
                await Answer(client);
            }
        }
    }

    private async Task Answer(TcpClient client)
    {
        string head;
        try
        {
            client.ReceiveTimeout = 5_000;
            head = await ReadHead(client.GetStream(), stopping.Token);
        }
        catch (Exception error) when (error is IOException or SocketException or OperationCanceledException or ObjectDisposedException)
        {
            return;
        }

        // The request line: `GET /callback?code=...&state=... HTTP/1.1`.
        var line = head.Split("\r\n", 2)[0];
        var parts = line.Split(' ');
        var target = parts.Length >= 2 ? parts[1] : "";
        var path = target.Split('?', 2)[0];
        var query = target.Contains('?', StringComparison.Ordinal) ? target.Split('?', 2)[1] : "";

        Landing landing;
        Result<string, Reason>? outcome = null;
        if (path != "/callback")
        {
            landing = Landing.NotHere;
        }
        else if (spent)
        {
            landing = Landing.AlreadyDone;
        }
        else
        {
            spent = true;
            var parsed = OAuth.ParseCallback(query);
            if (!parsed.IsOk)
            {
                landing = Landing.Refused;
                outcome = Result<string, Reason>.Err(parsed.Error);
            }
            else if (parsed.Value.State != expected)
            {
                landing = Landing.Mismatch;
                outcome = Result<string, Reason>.Err(new Reason.Unauthorised("the sign-in did not come back from the request Armory started"));
            }
            else
            {
                landing = Landing.SignedIn;
                outcome = Result<string, Reason>.Ok(parsed.Value.Code);
            }
        }

        var body = Encoding.UTF8.GetBytes(Page(landing));
        var status = landing == Landing.NotHere ? "404 Not Found" : "200 OK";
        var response = Encoding.ASCII.GetBytes($"HTTP/1.1 {status}\r\nServer: Armory\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {body.Length}\r\nConnection: close\r\n\r\n");
        try
        {
            var stream = client.GetStream();
            await stream.WriteAsync(response, stopping.Token);
            await stream.WriteAsync(body, stopping.Token);
            await stream.FlushAsync(stopping.Token);
        }
        catch (Exception error) when (error is IOException or SocketException or OperationCanceledException or ObjectDisposedException)
        {
            // The browser went away; the code still counts.
        }

        if (outcome is { } delivered)
        {
            deliver(delivered);
        }
    }

    /// <summary>The request head, up to the blank line. The body, if any, is not wanted.</summary>
    private static async Task<string> ReadHead(NetworkStream stream, CancellationToken cancellation)
    {
        var buffer = new byte[8192];
        var held = new MemoryStream();
        while (held.Length < buffer.Length)
        {
            var read = await stream.ReadAsync(buffer.AsMemory(0, buffer.Length - (int)held.Length), cancellation);
            if (read == 0)
            {
                break;
            }
            held.Write(buffer, 0, read);
            var text = Encoding.ASCII.GetString(held.GetBuffer(), 0, (int)held.Length);
            var end = text.IndexOf("\r\n\r\n", StringComparison.Ordinal);
            if (end >= 0)
            {
                return text[..end];
            }
        }
        return Encoding.ASCII.GetString(held.GetBuffer(), 0, (int)held.Length);
    }

    public void Dispose()
    {
        stopping.Cancel();
        listener.Stop();
        stopping.Dispose();
    }

    /// <summary>What the browser is told, in each of the cases it can land in.</summary>
    internal enum Landing
    {
        SignedIn,
        Mismatch,
        Refused,
        AlreadyDone,
        NotHere,
    }

    internal static string Heading(Landing landing) => landing switch
    {
        Landing.SignedIn => "Signed in",
        Landing.Mismatch => "That did not come from Armory",
        Landing.Refused => "Sign-in cancelled",
        Landing.AlreadyDone => "Already done",
        _ => "Nothing here",
    };

    internal static string Body(Landing landing) => landing switch
    {
        Landing.SignedIn => "You can close this tab and go back to Armory.",
        Landing.Mismatch => "The sign-in response did not match the one Armory started, so it has been ignored and nothing has been saved. Start again from Armory.",
        Landing.Refused => "Nothing has been saved. You can try again from Armory whenever you like.",
        Landing.AlreadyDone => "You can close this tab.",
        _ => "Armory is listening for a Battle.net sign-in, and this is not one.",
    };

    /// <summary>
    /// Render one of them. No external anything: no fonts, no scripts, no
    /// images. This is served to a browser by a process that is not a web
    /// server, and a page that fetches something can hang on a machine with
    /// no route out.
    /// </summary>
    internal static string Page(Landing landing) =>
        "<!doctype html><meta charset=utf-8><title>Armory</title>" +
        "<style>body{font-family:system-ui,sans-serif;margin:4rem auto;max-width:30rem;padding:0 1rem;line-height:1.5;color:#241f31;background:#fafafa}" +
        "@media(prefers-color-scheme:dark){body{color:#deddda;background:#1d1d20}}h1{font-size:1.3rem}</style>" +
        $"<h1>{Heading(landing)}</h1><p>{Body(landing)}</p>";
}
