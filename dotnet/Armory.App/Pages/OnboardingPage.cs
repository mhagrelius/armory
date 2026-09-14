using Armory.Blizzard;
using Armory.Client.Shell;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;

namespace Armory.App.Pages;

/// <summary>
/// Connecting a Battle.net client, or deciding to go without one. A single
/// 560px column. Battle.net has no PKCE and no public-client mode, so the
/// person registers their own client; the redirect port is fixed and
/// registered, and it is the one <see cref="OAuth.RedirectUri"/> names, not
/// the one the design mock shows.
/// </summary>
public sealed partial class OnboardingPage : Page
{
    private readonly Account account;
    private readonly MainWindow window;
    private readonly TextBlock waiting;
    private readonly InfoBar report;
    private readonly Button signIn;

    public OnboardingPage(Account account, MainWindow window)
    {
        this.account = account;
        this.window = window;
        var column = new StackPanel { Spacing = 16, MaxWidth = 560, HorizontalAlignment = HorizontalAlignment.Center, Margin = new Thickness(0, 40, 0, 40) };

        column.Children.Add(new TextBlock { Text = "Connect your Battle.net account", Style = (Style)Application.Current.Resources["PageTitle"] });
        column.Children.Add(Widgets.Text("Armory reads your characters through Blizzard's API. It needs a client of your own — Blizzard does not hand them out to applications.", "Secondary"));

        var one = new StackPanel { Spacing = 10 };
        one.Children.Add(Widgets.CardHead("1. Create an API client", "opens in your browser"));
        var sentence = new TextBlock { TextWrapping = TextWrapping.Wrap };
        sentence.Inlines.Add(new Microsoft.UI.Xaml.Documents.Run { Text = "Sign in at the " });
        var link = new Microsoft.UI.Xaml.Documents.Hyperlink { NavigateUri = new Uri("https://community.developer.battle.net/application") };
        link.Inlines.Add(new Microsoft.UI.Xaml.Documents.Run { Text = "Battle.net developer portal" });
        sentence.Inlines.Add(link);
        sentence.Inlines.Add(new Microsoft.UI.Xaml.Documents.Run { Text = " and create a client. Paste this redirect URI into it." });
        one.Children.Add(sentence);
        one.Children.Add(new TextBlock { Text = "Redirect URI", Style = (Style)Application.Current.Resources["Caption"] });
        var redirect = new Grid { ColumnSpacing = 8 };
        redirect.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        redirect.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        redirect.Children.Add(new TextBox { Text = OAuth.RedirectUri, IsReadOnly = true });
        var copy = Widgets.Standard("Copy");
        copy.Click += (_, _) =>
        {
            var package = new Windows.ApplicationModel.DataTransfer.DataPackage();
            package.SetText(OAuth.RedirectUri);
            Windows.ApplicationModel.DataTransfer.Clipboard.SetContent(package);
            window.Say("Redirect URI copied.");
        };
        Grid.SetColumn(copy, 1);
        redirect.Children.Add(copy);
        one.Children.Add(redirect);
        column.Children.Add(Widgets.Card(one));

        var two = new StackPanel { Spacing = 10 };
        two.Children.Add(Widgets.CardHead("2. Paste the client back here"));
        var clientId = new TextBox { Header = "Client ID", PlaceholderText = "Paste the client ID", Text = account.Settings.ClientId };
        // A blank secret field means "use the stored one". It cannot be
        // pre-filled without reading the secret out of the vault to display
        // it, and a field of dots from storage looks exactly like a field of
        // dots somebody typed; the header says whether one is held.
        var held = account.StoredSecret() is not null;
        var secret = new PasswordBox { Header = held ? "Client secret — already saved" : "Client secret", PlaceholderText = held ? "Leave blank to keep the saved one" : "Paste the client secret" };
        var region = new ComboBox { Header = "Region", HorizontalAlignment = HorizontalAlignment.Stretch };
        foreach (var candidate in RegionExtensions.All)
        {
            region.Items.Add($"{candidate.Label()} — where your characters are");
        }
        region.SelectedIndex = RegionExtensions.All.ToList().IndexOf(account.Settings.Region);
        two.Children.Add(clientId);
        two.Children.Add(secret);
        two.Children.Add(region);
        column.Children.Add(Widgets.Card(two));

        report = new InfoBar { IsOpen = false, Severity = InfoBarSeverity.Informational, IsClosable = false };
        column.Children.Add(report);

        var footer = new Grid { ColumnSpacing = 12 };
        footer.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        footer.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        footer.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        signIn = Widgets.Accent("Sign in with Battle.net");
        waiting = new TextBlock { Text = "", Style = (Style)Application.Current.Resources["Secondary"], VerticalAlignment = VerticalAlignment.Center };
        Grid.SetColumn(waiting, 1);
        signIn.Click += async (_, _) =>
        {
            await account.SignIn(clientId.Text, secret.Password, RegionExtensions.All[Math.Max(region.SelectedIndex, 0)]);
            SetBusy(account.SigningIn);
        };
        var skip = new HyperlinkButton { Content = "Use the addon only", VerticalAlignment = VerticalAlignment.Center };
        skip.Click += (_, _) => account.SkipApi();
        Grid.SetColumn(skip, 2);
        footer.Children.Add(signIn);
        footer.Children.Add(waiting);
        footer.Children.Add(skip);
        column.Children.Add(footer);

        column.Children.Add(Widgets.Text("Skipping is fine: the addon records evenings, zones and the counters the game does not keep. Without a Battle.net client, Armory cannot read your roster or the auction house.", "Caption"));

        Content = new ScrollViewer { Content = column, Padding = new Thickness(32, 0, 32, 0) };
        account.SignInReported += Report;
        SetBusy(account.SigningIn);
    }

    /// <summary>A sentence about the sign-in, or none. The waiting line clears with it once the flow has finished either way.</summary>
    private void Report(string? text)
    {
        _ = DispatcherQueue.TryEnqueue(() =>
        {
            report.Message = text ?? "";
            report.IsOpen = text is not null;
            report.Severity = text is not null && (text.StartsWith("Battle.net refused", StringComparison.Ordinal) || text.StartsWith("Sign-in did not finish", StringComparison.Ordinal) || text.Contains("could not listen", StringComparison.Ordinal))
                ? InfoBarSeverity.Error
                : InfoBarSeverity.Informational;
            SetBusy(account.SigningIn);
        });
    }

    private void SetBusy(bool busy)
    {
        waiting.Text = busy ? "Waiting for your browser…" : "";
        signIn.IsEnabled = !busy;
    }
}
