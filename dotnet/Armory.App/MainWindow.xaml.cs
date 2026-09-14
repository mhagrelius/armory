using Armory.App.Pages;
using Armory.App.Shell;
using Armory.Client.Shell;
using Armory.Collections;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;

namespace Armory.App;

/// <summary>
/// The window: a navigation pane in the GTK window's order, a content frame,
/// and the two banners. Every page is a main column of cards; the pane's
/// tallies are live counts from the collection data.
/// </summary>
public sealed partial class MainWindow : Window
{
    private readonly Account account;
    private readonly Dictionary<Kind, TextBlock> tallies = [];
    private readonly Dictionary<string, Page> pages = [];
    private readonly NavigationViewItem syncItem;
    private readonly ProgressRing syncRing;
    private int toastGeneration;

    public MainWindow(Account account)
    {
        this.account = account;
        InitializeComponent();
        ExtendsContentIntoTitleBar = true;
        SetTitleBar(TitleBar);
        // The GTK window's default, in logical pixels: Resize takes physical
        // ones, and at 125% a 1180 request is a 944 window.
        var scale = GetDpiForWindow(WinRT.Interop.WindowNative.GetWindowHandle(this)) / 96.0;
        // A preview may ask for a taller window: a page is painted at the
        // viewport's height and nothing scrolls, so the browser at the foot
        // of the Run page is invisible at 800.
        var height = int.TryParse(Environment.GetEnvironmentVariable("ARMORY_PREVIEW_HEIGHT"), out var asked) && asked > 0 ? asked : 800;
        AppWindow.Resize(new Windows.Graphics.SizeInt32((int)(1180 * scale), (int)(height * scale)));
        // The title bar and taskbar icon. The exe carries the same file for Explorer and the shortcut.
        var icon = Path.Combine(AppContext.BaseDirectory, "Assets", "Armory.ico");
        AppWindow.SetIcon(icon);
        // The same file in the title bar, so the window, the taskbar and the Start Menu agree.
        Mark.Source = new Microsoft.UI.Xaml.Media.Imaging.BitmapImage(new Uri(icon));
        BuildPane();
        (syncItem, syncRing) = BuildFooter();
        account.Toasted += Say;
        account.Noticed += SetNotice;
        account.Changed += RefreshTallies;
        account.OnboardingWanted += wanted => Open(wanted ? "onboarding" : "run");
        account.BusyChanged += busy => _ = DispatcherQueue.TryEnqueue(() => ShowBusy(busy));
        Navigation.SelectedItem = Navigation.MenuItems[0];
        if (account.NeedsOnboarding)
        {
            Open("onboarding");
        }
    }

    private void BuildPane()
    {
        foreach (var place in Places.First)
        {
            Navigation.MenuItems.Add(Item(place));
        }
        Navigation.MenuItems.Add(new NavigationViewItemHeader { Content = "Collection" });
        foreach (var place in Places.Collection)
        {
            Navigation.MenuItems.Add(Item(place));
        }
        Navigation.MenuItems.Add(new NavigationViewItemHeader { Content = "Account" });
        foreach (var place in Places.Account)
        {
            Navigation.MenuItems.Add(Item(place));
        }
    }

    /// <summary>
    /// The window-level commands, above Settings: the GTK header bar's sync
    /// button with its spinner, and the menu's About. A page header carries
    /// the page's own primary action; a sync is the whole account's, so it
    /// sits with the pane rather than with any one page.
    /// </summary>
    private (NavigationViewItem Sync, ProgressRing Ring) BuildFooter()
    {
        var ring = new ProgressRing { Width = 16, Height = 16, IsActive = false, Visibility = Visibility.Collapsed, VerticalAlignment = VerticalAlignment.Center };
        var label = new TextBlock { Text = "Sync now", VerticalAlignment = VerticalAlignment.Center };
        var content = new Grid
        {
            ColumnDefinitions = { new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) }, new ColumnDefinition { Width = GridLength.Auto } },
            Children = { label, ring },
        };
        Grid.SetColumn(ring, 1);
        var sync = new NavigationViewItem
        {
            Content = content,
            Tag = "sync",
            Icon = new FontIcon { Glyph = "" },
            SelectsOnInvoked = false,
        };
        ToolTipService.SetToolTip(sync, "Sync with Battle.net (Ctrl+R)");
        Navigation.FooterMenuItems.Add(sync);

        var about = new NavigationViewItem
        {
            Content = "About Armory",
            Tag = "about",
            Icon = new FontIcon { Glyph = "" },
            SelectsOnInvoked = false,
        };
        Navigation.FooterMenuItems.Add(about);
        return (sync, ring);
    }

    /// <summary>A request in flight: the bar over the content, the ring on the sync row, and the row disabled.</summary>
    private void ShowBusy(bool busy)
    {
        Busy.Visibility = busy ? Visibility.Visible : Visibility.Collapsed;
        syncRing.IsActive = busy;
        syncRing.Visibility = busy ? Visibility.Visible : Visibility.Collapsed;
        syncItem.IsEnabled = !busy;
    }

    private NavigationViewItem Item(Place place)
    {
        var item = new NavigationViewItem
        {
            Content = place.Title,
            Tag = place.Name,
            Icon = new FontIcon { Glyph = place.Glyph },
        };
        if (place.Collection is { } kind)
        {
            // A count on the row is the difference between a list of places
            // and a collection's standing at a glance.
            var tally = new TextBlock { Style = (Style)Application.Current.Resources["Caption"], VerticalAlignment = VerticalAlignment.Center };
            tallies[kind] = tally;
            item.InfoBadge = null;
            item.Content = new Grid
            {
                ColumnDefinitions = { new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) }, new ColumnDefinition { Width = GridLength.Auto } },
                Children = { new TextBlock { Text = place.Title, VerticalAlignment = VerticalAlignment.Center }, tally },
            };
            Grid.SetColumn(tally, 1);
        }
        return item;
    }

    /// <summary>The tallies on the four collection rows: owned of total, from the store.</summary>
    private async void RefreshTallies()
    {
        foreach (var kind in tallies.Keys.ToList())
        {
            var (catalogue, owned) = await account.Store.On(store => store.CollectiblesHeld(kind).Match(held => held, _ => ([], [])));
            if (catalogue.Count > 0)
            {
                tallies[kind].Text = $"{owned.Count}/{catalogue.Count}";
            }
        }
    }

    private void Navigation_SelectionChanged(NavigationView sender, NavigationViewSelectionChangedEventArgs args)
    {
        if (args.IsSettingsSelected)
        {
            Open("settings");
            return;
        }
        if (args.SelectedItem is NavigationViewItem { Tag: string name })
        {
            Open(name);
        }
    }

    /// <summary>The footer's two commands. Neither selects, so the open place stays open.</summary>
    private async void Navigation_ItemInvoked(NavigationView sender, NavigationViewItemInvokedEventArgs args)
    {
        switch (args.InvokedItemContainer?.Tag)
        {
            case "sync":
                await Sync();
                break;
            case "about":
                await ShowAbout();
                break;
        }
    }

    private async void Sync_Invoked(Microsoft.UI.Xaml.Input.KeyboardAccelerator sender, Microsoft.UI.Xaml.Input.KeyboardAcceleratorInvokedEventArgs args)
    {
        args.Handled = true;
        await Sync();
    }

    private void Quit_Invoked(Microsoft.UI.Xaml.Input.KeyboardAccelerator sender, Microsoft.UI.Xaml.Input.KeyboardAcceleratorInvokedEventArgs args)
    {
        args.Handled = true;
        Quit();
    }

    /// <summary>
    /// Sync with Battle.net, from the footer or Ctrl+R. The one operation the
    /// Settings page's own button runs; a press while a request is in flight
    /// is refused here as well as by the account.
    /// </summary>
    public Task Sync() => account.Busy ? Task.CompletedTask : account.Sync();

    /// <summary>
    /// Quit, through the same door the close button uses: closing the window
    /// raises <see cref="Window.Closed"/>, and that is where the thirty-day
    /// sweep runs. Exiting the application directly would skip it.
    /// </summary>
    public void Quit() => Close();

    /// <summary>About Armory, over the whole window.</summary>
    public async Task ShowAbout()
    {
        var dialog = new Dialogs.AboutDialog { XamlRoot = Shell.XamlRoot };
        await dialog.ShowAsync();
    }

    /// <summary>Show a place, keeping each page's own state when you leave and return.</summary>
    public void Open(string name)
    {
        if (!pages.TryGetValue(name, out var page))
        {
            page = Make(name);
            pages[name] = page;
        }
        // A page reached from another page (the Run's "Chronicle" link, a
        // character's evenings) moves the pane's selection with it. Selecting
        // raises SelectionChanged, which lands back here with the same name
        // and finds the page already shown.
        var item = Navigation.MenuItems.OfType<NavigationViewItem>().FirstOrDefault(candidate => candidate.Tag is string tag && tag == name);
        if (item is not null && !ReferenceEquals(Navigation.SelectedItem, item))
        {
            Navigation.SelectedItem = item;
        }
        if (ReferenceEquals(Shell.Content, page))
        {
            return;
        }
        Shell.Content = page;
        Search.PlaceholderText = page is ISearchable searchable ? searchable.SearchPlaceholder : "Search this page";
        Search.Visibility = page is ISearchable ? Visibility.Visible : Visibility.Collapsed;
        Caption.Text = name == "onboarding" ? "Armory — setup" : "Armory";
    }

    /// <summary>Open the Chronicle on one character's evenings, or on everyone's.</summary>
    public void OpenChronicle(string? displayName)
    {
        Open("chronicle");
        if (Shell.Content is ChroniclePage chronicle)
        {
            chronicle.ShowOnly(displayName);
        }
    }

    /// <summary>Open a character's page from the roster, with a way back.</summary>
    public void OpenCharacter(Armory.Roster.CharacterKey key)
    {
        var page = new CharacterPage(account, key, this);
        Shell.Content = page;
        Search.Visibility = Visibility.Collapsed;
        Caption.Text = "Armory";
    }

    private Page Make(string name) => name switch
    {
        "run" => new RunPage(account, this),
        "chronicle" => new ChroniclePage(account, this),
        "roster" => new RosterPage(account, this),
        "mounts" => new CollectionPage(account, Kind.Mount),
        "pets" => new CollectionPage(account, Kind.Pet),
        "toys" => new CollectionPage(account, Kind.Toy),
        "decor" => new CollectionPage(account, Kind.Decor),
        "zones" => new ZonesPage(account),
        "market" => new MarketPage(account, this),
        "onboarding" => new OnboardingPage(account, this),
        "settings" => new SettingsPage(account, this),
        "reputations" => new ReputationsPage(account),
        _ => new ComingPage(Places.Named(name)?.Title ?? name),
    };

    private void Search_TextChanged(AutoSuggestBox sender, AutoSuggestBoxTextChangedEventArgs args)
    {
        if (Shell.Content is ISearchable searchable)
        {
            searchable.Search(sender.Text);
        }
    }

    /// <summary>A sentence for a moment, at the bottom of the content.</summary>
    public void Say(string text)
    {
        Toast.Message = text;
        Toast.IsOpen = true;
        var mine = ++toastGeneration;
        _ = DispatcherQueue.TryEnqueue(async () =>
        {
            await Task.Delay(TimeSpan.FromSeconds(5));
            // A newer toast has taken the bar; leave it.
            if (mine == toastGeneration)
            {
                Toast.IsOpen = false;
            }
        });
    }

    /// <summary>A standing condition, or none.</summary>
    public void SetNotice(string? text)
    {
        Notice.Message = text ?? "";
        Notice.IsOpen = text is not null;
    }

    /// <summary>The Account &amp; Sharing dialog, over the whole window.</summary>
    public async Task ShowSharing()
    {
        var dialog = new Dialogs.SyncDialog(account) { XamlRoot = Shell.XamlRoot };
        await dialog.ShowAsync();
    }

    [System.Runtime.InteropServices.DllImport("user32.dll")]
    private static extern uint GetDpiForWindow(IntPtr window);
}

/// <summary>A page the pane's search box searches.</summary>
public interface ISearchable
{
    string SearchPlaceholder { get; }

    void Search(string text);
}
