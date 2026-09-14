using Armory.Client.Shell;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;

namespace Armory.App.Pages;

/// <summary>Settings: the journal server, the WoW install, and the way into Account &amp; Sharing.</summary>
public sealed partial class SettingsPage : Page
{
    public SettingsPage(Account account, MainWindow window)
    {
        var column = new StackPanel { Spacing = 16 };
        column.Children.Add(Widgets.Header("Settings", "What this machine reads, and where it writes"));

        var game = new StackPanel { Spacing = 10 };
        game.Children.Add(Widgets.CardHead("World of Warcraft"));
        var path = new TextBox { Header = "Install folder (the one holding WTF)", Text = account.Settings.WowPath ?? "", PlaceholderText = @"C:\Program Files (x86)\World of Warcraft\_retail_" };
        game.Children.Add(path);
        game.Children.Add(Widgets.Text(account.GameAccounts.Count == 0
            ? "No WTF/Account folder found there yet."
            : $"Reading the {account.Settings.WowAccount} account folder. Account & Sharing is where to change it.", "Caption"));
        column.Children.Add(Widgets.Card(game));

        column.Children.Add(Settings.JournalSection.Build(account, window));

        column.Children.Add(Settings.BattleNetSection.Build(account, window));

        var sharing = new StackPanel { Spacing = 10 };
        sharing.Children.Add(Widgets.CardHead("Account & Sharing", account.Settings.SyncUrl.Length == 0 ? "not sharing" : account.Settings.SyncUrl));
        var open = Widgets.Standard("Account & Sharing…");
        open.Click += async (_, _) => await window.ShowSharing();
        sharing.Children.Add(open);
        column.Children.Add(Widgets.Card(sharing));

        var save = Widgets.Accent("Save");
        save.Click += (_, _) =>
        {
            account.SaveSettings(account.Settings with
            {
                WowPath = path.Text.Trim().Length == 0 ? null : path.Text.Trim(),
            });
            account.StartWatching();
            window.Say("Settings saved.");
        };
        column.Children.Add(save);

        Content = new ScrollViewer { Content = column, Padding = (Thickness)Application.Current.Resources["PagePadding"] };
    }
}
