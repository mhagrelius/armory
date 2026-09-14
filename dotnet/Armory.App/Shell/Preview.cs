using System.Globalization;
using System.Runtime.InteropServices;
using System.Runtime.InteropServices.WindowsRuntime;
using System.Text;
using Armory.Addon;
using Armory.Client.Shell;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media.Imaging;
using Windows.Foundation;
using Windows.Graphics.Imaging;
using Windows.Storage.Streams;

namespace Armory.App.Shell;

/// <summary>
/// Paints the real pages to PNGs and exits. The port of
/// <c>examples/preview.rs</c>: this is how a UI change gets looked at, because
/// nothing else hands a screenshot to a non-interactive caller. Driven by the
/// <c>ARMORY_PREVIEW</c> environment variable naming the output directory,
/// with <c>ARMORY_PREVIEW_THEME=dark|light</c> choosing the theme and
/// <c>ARMORY_PREVIEW_HOLD=&lt;place&gt;</c> leaving the window open on one place
/// (with its dialog, when it has one) for a screen capture from outside, since
/// a rendered bitmap does not include the popup layer a dialog lives in.
/// </summary>
/// <remarks>
/// <para>
/// <c>ARMORY_PREVIEW_SAMPLE=1</c> paints the made-up account in
/// <see cref="Sample"/> instead of the one on this machine: a store in
/// memory seeded through the store's own writers, settings under a
/// throwaway folder, and the addon's read handed to the account so the run
/// is planned with goals in every bucket. The real store is never opened.
/// </para>
/// <para>
/// Every page is also measured against the width the window gives it — the
/// port of <c>tests/width.rs</c> — and one line per page goes to
/// <c>width.txt</c> in the output directory and to the console. A page over
/// budget makes the process exit non-zero, so the check can be scripted.
/// </para>
/// </remarks>
public static class Preview
{
    public const string Variable = "ARMORY_PREVIEW";

    /// <summary>The window's default width in logical pixels, from <see cref="MainWindow"/>'s resize.</summary>
    public const int Window = 1180;

    /// <summary>The navigation pane's <c>OpenPaneLength</c>: what the window keeps for itself at that width.</summary>
    public const int Pane = 260;

    /// <summary>How long to let a page settle before it is painted: the store reads land on the dispatcher.</summary>
    private static readonly TimeSpan Settle = TimeSpan.FromMilliseconds(1200);

    public static string? Directory => Environment.GetEnvironmentVariable(Variable) is { Length: > 0 } dir ? dir : null;

    /// <summary>Whether to paint the sample account rather than the real one.</summary>
    public static bool Sampled => Environment.GetEnvironmentVariable("ARMORY_PREVIEW_SAMPLE") == "1";

    /// <summary>What a page is allowed to need, and why: the default window less the open pane.</summary>
    public static int Budget => Window - Pane;

    /// <summary>
    /// The account the sample preview runs against. Settings under a
    /// throwaway folder — with the Rarity fixture installed beneath it, which
    /// is the only way the odds reach the account — a store in memory
    /// seeded with the sample, and no secrets. Nothing of the machine's own
    /// account is read or written.
    /// </summary>
    public static Account SampleAccount(Action<Func<Task>> dispatch)
    {
        var root = Path.Combine(Path.GetTempPath(), "armory-preview-sample");
        if (System.IO.Directory.Exists(root))
        {
            System.IO.Directory.Delete(root, recursive: true);
        }
        var wow = Path.Combine(root, "wow");
        System.IO.Directory.CreateDirectory(wow);
        Sample.InstallRarity(wow);
        var settings = Path.Combine(root, "settings.json");
        // Addon-only, so the pages do not open on onboarding; the journal
        // held back, so seeding the evenings does not start writing entries.
        new Armory.Settings.Settings { WowPath = wow, AddonOnly = true, JournalAutomatic = false }.Save(settings);

        var store = Armory.Store.Store.InMemory();
        var seeded = Sample.Seed(store);
        if (!seeded.IsOk)
        {
            throw new InvalidOperationException($"the sample could not be seeded: {seeded.Error}");
        }
        return new Account(new StoreWorker(store), new MemorySecrets(), settings, dispatch)
        {
            ArtDirectory = Path.Combine(root, "cache"),
        };
    }

    public static async Task Run(MainWindow window, Account account, string directory)
    {
        System.IO.Directory.CreateDirectory(directory);
        var theme = Environment.GetEnvironmentVariable("ARMORY_PREVIEW_THEME") == "light" ? ElementTheme.Light : ElementTheme.Dark;
        if (window.Content is FrameworkElement root)
        {
            root.RequestedTheme = theme;
        }
        var suffix = theme == ElementTheme.Dark ? "dark" : "light";
        var hold = Environment.GetEnvironmentVariable("ARMORY_PREVIEW_HOLD");

        if (Sampled)
        {
            // The addon's read, so the run is planned rather than merely
            // stored: the goal rows keep no evaluation, and a run page with
            // no progress bars would be a picture of the wrong state.
            await account.Collected(Result<Dump, ReadError>.Ok(Sample.Dump()));
        }
        // Pull a real account first when asked, so the pages are painted with
        // something in them. The address, account and token come from the
        // environment of this one process and go where the dialog would put
        // them; nothing is written into the repository.
        else if (Environment.GetEnvironmentVariable("ARMORY_PREVIEW_SYNC") is { Length: > 0 } sync
            && sync.Split('|') is [var address, var accountName]
            && Environment.GetEnvironmentVariable("ARMORY_PREVIEW_TOKEN") is { Length: > 0 } token)
        {
            await account.SaveSyncTarget(new Chosen(address, token, accountName, null));
            await account.ShareNow();
        }

        var widths = new List<(string Place, double Width, double Budget)>();
        var places = Places.All.Select(place => place.Name).Concat(["settings", "onboarding"]).ToList();
        foreach (var name in places)
        {
            window.Open(name);
            await Task.Delay(Settle);
            await Paint(window, Path.Combine(directory, $"{name}-{suffix}.png"));
            if (Measure(window) is { } measured)
            {
                widths.Add((name, measured.Width, measured.Budget));
            }
            await PaintEnd(window, Path.Combine(directory, $"{name}-{suffix}-end.png"));
        }
        var someone = Sampled ? Sample.Somechar : account.Roster.Characters.FirstOrDefault()?.Key;
        if (someone is not null)
        {
            window.OpenCharacter(someone);
            await Task.Delay(Settle);
            await Paint(window, Path.Combine(directory, $"character-{suffix}.png"));
            if (Measure(window) is { } measured)
            {
                widths.Add(("character", measured.Width, measured.Budget));
            }
            await PaintEnd(window, Path.Combine(directory, $"character-{suffix}-end.png"));
        }
        var over = Report(widths, Path.Combine(directory, "width.txt"));

        if (hold is { Length: > 0 })
        {
            window.Open(hold == "sharing" ? "settings" : hold);
            await Task.Delay(Settle);
            if (hold == "sharing")
            {
                _ = window.ShowSharing();
            }
            return;
        }
        // Main returns void, so the process takes this as its exit code.
        Environment.ExitCode = over ? 1 : 0;
        Application.Current.Exit();
    }

    /// <summary>
    /// How wide the open page wants to be against what the frame gives it.
    /// The port of <c>tests/width.rs</c>: a page needing more than the
    /// content pane overflows to the right, and nothing in the toolkit says
    /// so. The page's scroll viewer would clamp to its constraint, so the
    /// element measured is what sits inside it, at the width it actually
    /// has, and its desired width is what it cannot shrink below.
    /// </summary>
    /// <summary>
    /// The foot of a page, when asked with <c>ARMORY_PREVIEW_END=1</c>. A
    /// paint is the viewport and the window cannot be made taller than the
    /// screen, so a page longer than one screen — the Run page with its goal
    /// browser at the bottom — has a second picture, scrolled to the end.
    /// </summary>
    private static async Task PaintEnd(MainWindow window, string path)
    {
        if (Environment.GetEnvironmentVariable("ARMORY_PREVIEW_END") != "1")
        {
            return;
        }
        if (window.Content is not FrameworkElement root || root.FindName("Shell") is not Frame { Content: Page { Content: ScrollViewer scroller } })
        {
            return;
        }
        if (scroller.ScrollableHeight <= 0)
        {
            return;
        }
        scroller.ChangeView(null, scroller.ScrollableHeight, null, disableAnimation: true);
        await Task.Delay(Settle);
        await Paint(window, path);
        scroller.ChangeView(null, 0, null, disableAnimation: true);
    }

    private static (double Width, double Budget)? Measure(MainWindow window)
    {
        if (window.Content is not FrameworkElement root || root.FindName("Shell") is not Frame frame || frame.Content is not Page page)
        {
            return null;
        }
        var budget = frame.ActualWidth > 0 ? frame.ActualWidth : Budget;
        FrameworkElement element;
        var available = budget;
        switch (page.Content)
        {
            case ScrollViewer { Content: FrameworkElement inner } scroller:
                element = inner;
                available -= scroller.Padding.Left + scroller.Padding.Right;
                break;
            case FrameworkElement content:
                element = content;
                break;
            default:
                return null;
        }
        element.Measure(new Size(available, double.PositiveInfinity));
        var width = element.DesiredSize.Width + (budget - available);
        // Put the live layout back: the measure above was a question, not a change.
        element.InvalidateMeasure();
        frame.UpdateLayout();
        return (width, budget);
    }

    /// <summary>One line per page, to a file beside the pictures and to whatever console started this. True when any page is over.</summary>
    private static bool Report(IReadOnlyList<(string Place, double Width, double Budget)> widths, string path)
    {
        var over = false;
        var lines = new StringBuilder();
        foreach (var (place, width, budget) in widths)
        {
            var fits = width <= budget + 0.5;
            over |= !fits;
            lines.AppendLine(string.Create(CultureInfo.InvariantCulture, $"width {(fits ? "ok  " : "OVER")} {place,-12} {Math.Ceiling(width)}/{Math.Floor(budget)}"));
        }
        File.WriteAllText(path, lines.ToString());
        // A Windows app has no console of its own; the one that launched it
        // will do, when there is one.
        if (AttachConsole(unchecked((uint)-1)))
        {
            using var stdout = new StreamWriter(Console.OpenStandardOutput()) { AutoFlush = true };
            stdout.Write(lines.ToString());
        }
        return over;
    }

    private static async Task Paint(MainWindow window, string path)
    {
        if (window.Content is not UIElement element)
        {
            return;
        }
        var bitmap = new RenderTargetBitmap();
        await bitmap.RenderAsync(element);
        var pixels = await bitmap.GetPixelsAsync();
        using var stream = new InMemoryRandomAccessStream();
        var encoder = await BitmapEncoder.CreateAsync(BitmapEncoder.PngEncoderId, stream);
        encoder.SetPixelData(BitmapPixelFormat.Bgra8, BitmapAlphaMode.Premultiplied, (uint)bitmap.PixelWidth, (uint)bitmap.PixelHeight, 96, 96, WindowsRuntimeBufferExtensions.ToArray(pixels));
        await encoder.FlushAsync();
        using var file = File.Create(path);
        stream.Seek(0);
        using var reader = new DataReader(stream.GetInputStreamAt(0));
        await reader.LoadAsync((uint)stream.Size);
        var bytes = new byte[stream.Size];
        reader.ReadBytes(bytes);
        await file.WriteAsync(bytes);
    }

    [DllImport("kernel32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool AttachConsole(uint processId);
}
