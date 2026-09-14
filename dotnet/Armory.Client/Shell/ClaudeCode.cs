using System.ComponentModel;
using System.Diagnostics;
using System.Text;
using Armory.Blizzard;
using Armory.Chronicle;

namespace Armory.Client.Shell;

/// <summary>
/// Runs the <c>claude</c> command-line for the journal. The subprocess
/// equivalent of <see cref="Armory.Client.Blizzard.Http"/>: the core says what to run
/// (<see cref="Command"/>) and this performs it, converting everything a
/// process can do wrong into a <see cref="Reason"/> at this one seam.
/// </summary>
/// <remarks>
/// Two things are deliberate about the environment the child gets. An API
/// key is withheld, because print mode uses one ahead of the login when it
/// finds one and every entry would then be billed per token rather than
/// drawn on the subscription this path exists to use. And the working
/// directory is an empty folder of Armory's own, because the CLI reads
/// instructions out of whatever directory it is started in, and an evening's
/// entry has no business being written under some project's rules.
/// </remarks>
public static class ClaudeCode
{
    private static readonly UTF8Encoding Utf8 = new(encoderShouldEmitUTF8Identifier: false);

    /// <summary>What must not reach the child: either would be used instead of the login, and billed.</summary>
    private static readonly string[] Withheld = ["ANTHROPIC_API_KEY", "ANTHROPIC_AUTH_TOKEN"];

    /// <summary>
    /// Run one command to completion and hand back its standard output.
    /// Standard output is returned whatever the exit code, because the CLI
    /// reports its own failures as a result object with <c>is_error</c> set
    /// and a non-zero exit, and that object says more than the number does.
    /// </summary>
    public static async Task<Result<byte[], Reason>> Run(Command command, string workingDirectory, TimeSpan timeout, CancellationToken cancellation = default)
    {
        try
        {
            Directory.CreateDirectory(workingDirectory);
        }
        catch (Exception error) when (error is IOException or UnauthorizedAccessException)
        {
            return Result<byte[], Reason>.Err(new Reason.Network($"could not make {workingDirectory}: {error.Message}"));
        }

        Process? process = null;
        foreach (var program in Candidates(command.Program))
        {
            var info = new ProcessStartInfo(program)
            {
                UseShellExecute = false,
                CreateNoWindow = true,
                RedirectStandardInput = true,
                RedirectStandardOutput = true,
                RedirectStandardError = true,
                StandardInputEncoding = Utf8,
                StandardOutputEncoding = Utf8,
                StandardErrorEncoding = Utf8,
                WorkingDirectory = workingDirectory,
            };
            foreach (var argument in command.Arguments)
            {
                info.ArgumentList.Add(argument);
            }
            foreach (var name in Withheld)
            {
                info.Environment.Remove(name);
            }
            try
            {
                process = Process.Start(info);
                if (process is not null)
                {
                    break;
                }
            }
            catch (Win32Exception)
            {
                // Not there under that name; the next candidate may be.
            }
        }
        if (process is null)
        {
            return Result<byte[], Reason>.Err(new Reason.NotConfigured(
                $"{command.Program} is not installed, or is not on PATH for this application"));
        }

        using (process)
        {
            using var clock = CancellationTokenSource.CreateLinkedTokenSource(cancellation);
            clock.CancelAfter(timeout);
            try
            {
                // Both pipes are drained before stdin is written: a child
                // that fills stdout while its parent is still writing stdin
                // is a deadlock with a pipe buffer in the middle.
                var stdout = process.StandardOutput.ReadToEndAsync(clock.Token);
                var stderr = process.StandardError.ReadToEndAsync(clock.Token);
                await process.StandardInput.WriteAsync(command.Stdin.AsMemory(), clock.Token);
                process.StandardInput.Close();
                await process.WaitForExitAsync(clock.Token);
                var output = await stdout;
                var complaint = (await stderr).Trim();
                if (output.Trim().Length > 0)
                {
                    return Result<byte[], Reason>.Ok(Utf8.GetBytes(output));
                }
                return Result<byte[], Reason>.Err(new Reason.Declined(complaint.Length > 0
                    ? complaint
                    : $"{command.Program} exited with code {process.ExitCode} and said nothing"));
            }
            catch (OperationCanceledException) when (!cancellation.IsCancellationRequested)
            {
                Stop(process);
                return Result<byte[], Reason>.Err(new Reason.Timeout());
            }
            catch (OperationCanceledException)
            {
                Stop(process);
                return Result<byte[], Reason>.Err(new Reason.Declined("cancelled"));
            }
            catch (IOException error)
            {
                Stop(process);
                return Result<byte[], Reason>.Err(new Reason.Network(error.Message));
            }
        }
    }

    /// <summary>
    /// Where to look. PATH first, which is where the installer puts it; then
    /// the installer's own directory, for an application launched from a
    /// shortcut whose environment predates the install.
    /// </summary>
    private static IEnumerable<string> Candidates(string program)
    {
        yield return program;
        if (Path.IsPathRooted(program))
        {
            yield break;
        }
        var home = Environment.GetFolderPath(Environment.SpecialFolder.UserProfile);
        if (home.Length == 0)
        {
            yield break;
        }
        var installed = Path.Combine(home, ".local", "bin", OperatingSystem.IsWindows() ? $"{program}.exe" : program);
        if (File.Exists(installed))
        {
            yield return installed;
        }
    }

    private static void Stop(Process process)
    {
        try
        {
            if (!process.HasExited)
            {
                process.Kill(entireProcessTree: true);
            }
        }
        catch (Exception error) when (error is InvalidOperationException or Win32Exception)
        {
            // Already gone, which is what was wanted.
        }
    }
}
