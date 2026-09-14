using Armory.App.Shell;
using Armory.Client.Shell;
using Microsoft.UI.Dispatching;
using Microsoft.UI.Xaml;

namespace Armory.App;

public partial class App : Application
{
    private MainWindow? window;

    public App()
    {
        InitializeComponent();
        // A XAML crash otherwise dies as a stowed exception with nothing in
        // the event log but a fault offset. The GTK build writes to stderr;
        // a Windows app has none, so it is a file beside the store.
        UnhandledException += (_, error) =>
        {
            try
            {
                Directory.CreateDirectory(Paths.DataDir);
                File.AppendAllText(Paths.LogPath, $"{DateTimeOffset.Now:O} {error.Message}{Environment.NewLine}{error.Exception}{Environment.NewLine}");
            }
            catch (IOException)
            {
            }
            // Under the preview a page that throws is a finding, not the end
            // of the run: the log above is where the finding is read, and
            // the other pages still get painted.
            if (Preview.Directory is not null)
            {
                error.Handled = true;
            }
        };
    }

    /// <summary>The account this process holds. One per process, like the GTK application object.</summary>
    public static Account Account { get; private set; } = null!;

    protected override void OnLaunched(LaunchActivatedEventArgs args)
    {
        var dispatcher = DispatcherQueue.GetForCurrentThread();
        Directory.CreateDirectory(Paths.DataDir);
        void Dispatch(Func<Task> work) => dispatcher.TryEnqueue(() => _ = work());
        if (Preview.Directory is not null && Preview.Sampled)
        {
            // The made-up account, in memory: the real store is not opened.
            Account = Preview.SampleAccount(Dispatch);
        }
        else
        {
            // An in-memory fallback means a machine with an unwritable data
            // directory still runs; it just forgets.
            var store = Armory.Store.Store.Open(Paths.StorePath).Match(opened => opened, _ => Armory.Store.Store.InMemory());
            Account = new Account(
                new StoreWorker(store),
                new VaultSecrets(),
                Paths.SettingsPath,
                Dispatch);
        }

        Account.OpenBrowser = url => Windows.System.Launcher.LaunchUriAsync(new Uri(url)).AsTask();
        window = new MainWindow(Account);
        window.Closed += async (_, _) =>
        {
            // The thirty-day sweep, on the way out rather than on the way in.
            await Account.Shutdown();
            Account.Dispose();
        };
        window.Activate();
        _ = Launch(window);
    }

    private static async Task Launch(MainWindow window)
    {
        await Account.Restore();
        if (Preview.Directory is { } preview)
        {
            // Paint and leave; sharing is not started, so a preview never
            // pushes anything.
            await Preview.Run(window, Account, preview);
            return;
        }
        await Account.StartSharing();
    }
}
