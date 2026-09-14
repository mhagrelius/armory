using System.Globalization;
using Armory.App.Dialogs;
using Armory.Client.Shell;
using Armory.Collections;
using Armory.Market;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using Microsoft.UI.Xaml.Shapes;

namespace Armory.App.Pages;

/// <summary>
/// The auction house in three tabs. Browse is the whole market as it stands
/// now, ordered by market size with one row's book beside it. Watching is
/// the items whose history is being kept, a chart each. Crafting is what to
/// make, what to sell, and what to buy before it is gone, ranked by margin
/// times what is actually moving. The port of <c>ui/market_page.rs</c>
/// against the handoff's Market screen; the handoff cut Browse and Crafting,
/// and the GTK page wins.
/// </summary>
public sealed partial class MarketPage : Page, ISearchable
{
    /// <summary>How many rows the browser draws at once. Somebody who has not narrowed past this has not found what they wanted yet.</summary>
    public const int BrowseShown = 500;

    private const int ShownOffers = 6;
    private const int RankedShown = 12;

    private readonly Account account;
    private readonly MainWindow window;
    private readonly StackPanel column = new() { Spacing = 16 };
    private string tab = "browse";
    private string needle = "";
    private MarketView view = new();
    private Listed? selected;
    private bool drawing;
    private bool again;

    public MarketPage(Account account, MainWindow window)
    {
        this.account = account;
        this.window = window;
        Content = new ScrollViewer { Content = column, Padding = (Thickness)Application.Current.Resources["PagePadding"] };
        account.Changed += Reload;
        // The name backfill spends its budget on the rows in front of somebody.
        account.NamesWanted = WantsNames;
        Reload();
    }

    public string SearchPlaceholder => "Search the market";

    public void Search(string text)
    {
        needle = text;
        if (tab != "browse")
        {
            tab = "browse";
        }
        Redraw();
    }

    /// <summary>
    /// Which items on the page still have no name, in the page's own order:
    /// the budget is a hundred and fifty names a sync against a market of
    /// tens of thousands, and it has to be spent on what somebody is looking at.
    /// </summary>
    public List<long> WantsNames(int budget) => MarketProse.WantsNames(view.Market, needle, budget);

    private async void Reload()
    {
        // One read at a time; a change that lands mid-read queues one more.
        if (drawing)
        {
            again = true;
            return;
        }
        drawing = true;
        try
        {
            do
            {
                again = false;
                view = await account.MarketNow();
            }
            while (again);
        }
        finally
        {
            drawing = false;
        }
        if (selected is { } was)
        {
            selected = view.Market.FirstOrDefault(listed => listed.ItemId == was.ItemId);
        }
        Redraw();
    }

    private void Redraw()
    {
        column.Children.Clear();

        var watchItem = Widgets.Accent("Watch an item", "");
        watchItem.Click += async (_, _) => await WatchDialog.Items(XamlRoot, account.SearchItems, account.WatchItem);
        var addRealm = Widgets.Standard("Add a realm");
        addRealm.Click += async (_, _) => await AddRealm();
        var browsingName = view.Browsing == 0 ? "Region-wide commodities" : view.Realms.FirstOrDefault(realm => realm.Id == view.Browsing).Name ?? "Region-wide commodities";
        column.Children.Add(Widgets.Header("Market", string.Create(CultureInfo.InvariantCulture, $"{browsingName} · {view.Market.Count:N0} items on the market · whole file replaced hourly"), watchItem, addRealm));

        var selector = new SelectorBar();
        var tabs = new[] { ("browse", "Browse"), ("watching", view.Quotes.Count == 0 ? "Watching" : string.Create(CultureInfo.InvariantCulture, $"Watching {view.Quotes.Select(quote => quote.ItemId).Distinct().Count()}")), ("crafting", "Crafting") };
        foreach (var (name, title) in tabs)
        {
            selector.Items.Add(new SelectorBarItem { Text = title, Tag = name, IsSelected = name == tab });
        }
        selector.SelectionChanged += (sender, _) =>
        {
            var chosen = sender.SelectedItem?.Tag as string ?? "browse";
            if (chosen == tab)
            {
                return;
            }
            tab = chosen;
            DispatcherQueue.TryEnqueue(Redraw);
        };
        column.Children.Add(selector);

        switch (tab)
        {
            case "watching":
                DrawWatching();
                break;
            case "crafting":
                DrawCrafting();
                break;
            default:
                DrawBrowse();
                break;
        }
    }

    private async Task AddRealm()
    {
        var choices = await account.RealmChoices();
        if (!choices.IsOk)
        {
            window.Say(choices.Error);
            return;
        }
        await WatchDialog.Realms(XamlRoot, choices.Value.Realms, choices.Value.Mine, account.WatchRealm);
    }

    // -- browse ---------------------------------------------------------------

    private static Grid Columns()
    {
        var grid = new Grid { ColumnSpacing = 12 };
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        // Narrower than the GTK page's columns: the window is 1180 wide with a
        // 200 pane, and the item name is the point of the list, so the fixed
        // columns must not squeeze it.
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(84) });
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(100) });
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(96) });
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(84) });
        return grid;
    }

    private void DrawBrowse()
    {
        // Which market is being read, and the way to read another one.
        var picker = new ComboBox { MinWidth = 240 };
        picker.Items.Add("Region-wide commodities");
        foreach (var (_, name) in view.Realms)
        {
            picker.Items.Add(name);
        }
        picker.SelectedIndex = view.Browsing == 0 ? 0 : Math.Max(view.Realms.FindIndex(realm => realm.Id == view.Browsing) + 1, 0);
        picker.SelectionChanged += (_, _) =>
        {
            var chosen = picker.SelectedIndex <= 0 ? 0 : view.Realms[picker.SelectedIndex - 1].Id;
            if (chosen == view.Browsing)
            {
                return;
            }
            account.Browsing = chosen;
            DispatcherQueue.TryEnqueue(Reload);
        };
        column.Children.Add(picker);

        var total = view.Market.Count;
        var rows = MarketProse.Ordered(view.Market, needle);
        var shown = rows.Take(BrowseShown).ToList();

        var split = new Grid { ColumnSpacing = 16 };
        split.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        split.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(312) });

        var table = new StackPanel();
        var head = Columns();
        head.Margin = new Thickness(0, 0, 0, 6);
        var labels = new[] { "Item", "Cheapest", "Listed", "Market size ▾", "Selling" };
        for (var i = 0; i < labels.Length; i++)
        {
            var label = new TextBlock { Text = labels[i], Style = (Style)Application.Current.Resources["Caption"], HorizontalAlignment = i == 0 ? HorizontalAlignment.Left : HorizontalAlignment.Right };
            if (i == 3)
            {
                // The caret marks the one ordering there is: the median unit
                // price times the quantity listed, which is how much market
                // is actually here.
                label.Foreground = Widgets.AccentBrush;
            }
            Grid.SetColumn(label, i);
            head.Children.Add(label);
        }
        table.Children.Add(head);
        if (total == 0)
        {
            table.Children.Add(Widgets.Text("Nothing on this market yet. The region-wide commodities arrive with the first sync after a sign-in; a realm's own auctions once it is added.", "Secondary"));
        }
        foreach (var listed in shown)
        {
            table.Children.Add(BrowseRow(listed));
        }
        var searching = needle.Trim().Length > 0;
        var footer = (searching, rows.Count > BrowseShown) switch
        {
            (false, true) => string.Create(CultureInfo.InvariantCulture, $"Showing {shown.Count:N0} of {total:N0} — search to reach the rest"),
            (false, false) => string.Create(CultureInfo.InvariantCulture, $"Showing {shown.Count:N0} of {total:N0}"),
            (true, true) => string.Create(CultureInfo.InvariantCulture, $"{rows.Count:N0} of {total:N0} match — narrow it to reach the rest"),
            (true, false) => string.Create(CultureInfo.InvariantCulture, $"{rows.Count:N0} of {total:N0} match"),
        };
        table.Children.Add(new TextBlock { Text = footer, Style = (Style)Application.Current.Resources["Caption"], Margin = new Thickness(0, 10, 0, 0) });
        split.Children.Add(Widgets.Card(table));

        var rail = BrowseRail();
        Grid.SetColumn(rail, 1);
        split.Children.Add(rail);
        column.Children.Add(split);
    }

    private Border BrowseRow(Listed listed)
    {
        var grid = Columns();
        // An item the auction house has only ever given an id for. Said in
        // the mono face rather than hidden: an id is what somebody pasting
        // from a wiki has, and it is searchable.
        var name = new TextBlock { Text = listed.Title(), TextTrimming = TextTrimming.CharacterEllipsis, VerticalAlignment = VerticalAlignment.Center };
        if (listed.Name is null)
        {
            name.Foreground = Widgets.Tertiary;
            name.FontFamily = Mono;
        }
        grid.Children.Add(name);
        var cells = new[]
        {
            (MarketProse.Gold(listed.Cheapest), false),
            (string.Create(CultureInfo.InvariantCulture, $"{listed.Quantity:N0} in {listed.Listings:N0}"), false),
            (MarketProse.Gold(listed.Depth()), true),
            // "not watched" is the honest state and the argument for watching:
            // only a watched item has the history this column reads.
            (MarketProse.Selling(listed), listed.SpanHours > 0),
        };
        for (var i = 0; i < cells.Length; i++)
        {
            var (text, accent) = cells[i];
            var cell = new TextBlock { Text = text, FontFamily = Mono, HorizontalAlignment = HorizontalAlignment.Right, VerticalAlignment = VerticalAlignment.Center, Style = (Style)Application.Current.Resources["Caption"] };
            if (accent)
            {
                cell.Foreground = Widgets.AccentBrush;
            }
            else if (i == 3)
            {
                cell.Foreground = Widgets.Tertiary;
            }
            Grid.SetColumn(cell, i + 1);
            grid.Children.Add(cell);
        }
        var row = new Border { Style = (Style)Application.Current.Resources["RowDivider"], Child = grid };
        if (selected?.ItemId == listed.ItemId)
        {
            row.Background = (Brush)Application.Current.Resources["AccentFillColorSecondaryBrush"];
            row.Background.Opacity = 0.18;
        }
        row.PointerPressed += (_, _) =>
        {
            if (selected?.ItemId == listed.ItemId)
            {
                return;
            }
            selected = listed;
            DispatcherQueue.TryEnqueue(Redraw);
        };
        return row;
    }

    /// <summary>The selected row's order book: the whole reason <c>Depth</c> keeps more than a floor.</summary>
    private Border BrowseRail()
    {
        var rail = new StackPanel { Spacing = 10 };
        if (selected is not { } listed)
        {
            rail.Children.Add(Widgets.CardHead("Nothing chosen"));
            rail.Children.Add(Widgets.Text("Choose a row and its book — what one costs, what a tenth of the stock costs, and where the real middle is — is drawn here.", "Secondary"));
            return Widgets.Card(rail);
        }
        rail.Children.Add(Widgets.CardHead(listed.Title(), "The shape of the book, not just its first row"));
        rail.Children.Add(BookLine("One costs", MarketProse.Gold(listed.Cheapest), true));
        rail.Children.Add(BookLine("Through the cheap tenth", MarketProse.Gold(listed.Tenth)));
        rail.Children.Add(BookLine("The real middle", MarketProse.Gold(listed.Median)));
        rail.Children.Add(BookLine("Units listed", listed.Quantity.ToString("N0", CultureInfo.InvariantCulture)));
        rail.Children.Add(BookLine("Across auctions", listed.Listings.ToString("N0", CultureInfo.InvariantCulture)));

        // The bar is the median, cut where the floor and the cheap tenth
        // fall. Mostly accent is a book with no spread in it; mostly track is
        // a floor that one hopeful listing is holding down.
        var middle = Math.Max(listed.Median, 1);
        var floor = Math.Clamp(listed.Cheapest / (double)middle, 0.0, 1.0);
        var tenth = Math.Clamp(listed.Tenth / (double)middle, floor, 1.0);
        var depth = new Grid { Height = 6, Margin = new Thickness(0, 6, 0, 0) };
        depth.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(Math.Max(floor, 0.001), GridUnitType.Star) });
        depth.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(Math.Max(tenth - floor, 0.001), GridUnitType.Star) });
        depth.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(Math.Max(1.0 - tenth, 0.001), GridUnitType.Star) });
        var segments = new[] { (Widgets.AccentBrush, 1.0), (Widgets.AccentBrush, 0.5), ((Brush)Application.Current.Resources["ControlStrongFillColorDefaultBrush"], 0.6) };
        for (var i = 0; i < 3; i++)
        {
            var segment = new Border { Background = segments[i].Item1, Opacity = segments[i].Item2, CornerRadius = new CornerRadius(2), Margin = new Thickness(i == 0 ? 0 : 1, 0, 0, 0) };
            Grid.SetColumn(segment, i);
            depth.Children.Add(segment);
        }
        rail.Children.Add(depth);
        var captions = new Grid();
        captions.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        captions.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        captions.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        var names = new[] { ("One costs", HorizontalAlignment.Left), ("Cheap tenth", HorizontalAlignment.Center), ("The middle", HorizontalAlignment.Right) };
        for (var i = 0; i < 3; i++)
        {
            var caption = new TextBlock { Text = names[i].Item1, Style = (Style)Application.Current.Resources["Caption"], HorizontalAlignment = names[i].Item2 };
            Grid.SetColumn(caption, i);
            captions.Children.Add(caption);
        }
        rail.Children.Add(captions);

        if (view.Watching.Contains(listed.ItemId))
        {
            rail.Children.Add(new TextBlock { Text = "Watched — its history is being kept", Style = (Style)Application.Current.Resources["Caption"], Foreground = Widgets.AccentBrush, Margin = new Thickness(0, 8, 0, 0) });
        }
        else
        {
            var watch = Widgets.Accent("Watch this item");
            watch.Margin = new Thickness(0, 8, 0, 0);
            var id = listed.ItemId;
            var name = listed.Title();
            watch.Click += async (_, _) => await account.WatchItem(id, name);
            rail.Children.Add(watch);
            rail.Children.Add(Widgets.Text("Nothing before you ask can be recovered. Blizzard publishes no history at all, so the first snapshot after you press this is where yours starts.", "Caption"));
        }
        return Widgets.Card(rail);
    }

    private static Grid BookLine(string name, string value, bool large = false)
    {
        var line = new Grid { ColumnSpacing = 8 };
        line.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        line.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        line.Children.Add(new TextBlock { Text = name, Style = (Style)Application.Current.Resources["Caption"], VerticalAlignment = VerticalAlignment.Bottom });
        var figure = new TextBlock { Text = value, FontFamily = Mono, Style = (Style)Application.Current.Resources[large ? "Figure" : "Caption"], HorizontalAlignment = HorizontalAlignment.Right };
        Grid.SetColumn(figure, 1);
        line.Children.Add(figure);
        return line;
    }

    // -- watching -------------------------------------------------------------

    private void DrawWatching()
    {
        var split = new Grid { ColumnSpacing = 16 };
        split.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        split.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(312) });

        var main = new StackPanel { Spacing = 16 };
        if (view.Quotes.Count == 0)
        {
            var empty = new StackPanel { Spacing = 12 };
            empty.Children.Add(Widgets.CardHead("Nothing watched yet"));
            empty.Children.Add(Widgets.Text(view.Realms.Count == 0
                ? "Browse shows the whole market as it stands right now, and costs nothing — but only a watched item gets a history. Blizzard publishes none at all, so the first snapshot after you ask is where yours starts, and the days before it cannot be recovered. Add a realm to follow gear, pets and recipes there too."
                : "Browse shows the whole market as it stands right now. Watching an item is what starts recording its price — Blizzard publishes no history, so the first snapshot after you ask is where yours starts, and a trend needs a few hours before it means anything.", "Secondary"));
            var add = Widgets.Accent("Watch an Item");
            add.HorizontalAlignment = HorizontalAlignment.Left;
            add.Click += async (_, _) => await WatchDialog.Items(XamlRoot, account.SearchItems, account.WatchItem);
            empty.Children.Add(add);
            main.Children.Add(Widgets.Card(empty));
        }
        else
        {
            var groups = new List<(long Id, string Name)> { (0, "Region-wide commodities") };
            groups.AddRange(view.Realms);
            // A quote on a realm that is no longer followed still has a
            // history and still has to be reachable.
            foreach (var quote in view.Quotes.Where(quote => !groups.Any(group => group.Id == quote.Realm)))
            {
                groups.Add((quote.Realm, quote.RealmName));
            }
            foreach (var (realm, name) in groups)
            {
                var rows = view.Quotes.Where(quote => quote.Realm == realm).OrderBy(quote => quote.Name, StringComparer.OrdinalIgnoreCase).ToList();
                if (rows.Count == 0)
                {
                    continue;
                }
                var cards = new StackPanel();
                cards.Children.Add(Widgets.CardHead(name));
                foreach (var quote in rows)
                {
                    cards.Children.Add(WatchRow(quote));
                }
                main.Children.Add(Widgets.Card(cards));
            }
        }
        split.Children.Add(main);
        var rail = WatchingRail();
        Grid.SetColumn(rail, 1);
        split.Children.Add(rail);
        column.Children.Add(split);
    }

    /// <summary>One watched item: what it has been doing, and where it ended up.</summary>
    private Border WatchRow(Quote quote)
    {
        var grid = new Grid { ColumnSpacing = 12 };
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(86) });
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });

        var text = new StackPanel { Spacing = 4, VerticalAlignment = VerticalAlignment.Center };
        var title = new TextBlock { Text = quote.Name, Style = (Style)Application.Current.Resources["BodyStrongTextBlockStyle"], TextTrimming = TextTrimming.CharacterEllipsis };
        text.Children.Add(title);
        var days = quote.Days();
        var since = quote.Since?.ToLocalTime().ToString("d MMMM", CultureInfo.InvariantCulture) ?? "today";
        var meta = new TextBlock
        {
            Text = days >= Quote.Term
                // The ceiling is a term of the licence, and a series sitting on it is at the oldest reading there will ever be.
                ? string.Create(CultureInfo.InvariantCulture, $"Watching since {since} · {Quote.Term} days held — the term's ceiling")
                : string.Create(CultureInfo.InvariantCulture, $"Watching since {since} · {Chronicle.Prose.Plural(days, "day", "days")} observed"),
            Style = (Style)Application.Current.Resources["Caption"],
        };
        text.Children.Add(meta);
        if (quote.History.Count < 2)
        {
            meta.Foreground = Widgets.AccentBrush;
            text.Children.Add(Widgets.Text("Two readings is not a trend. Nothing before you asked can be recovered.", "Caption"));
        }
        else
        {
            var figures = new List<string>();
            if (quote.Latest is { } latest)
            {
                figures.Add(string.Create(CultureInfo.InvariantCulture, $"{latest.Quantity:N0} listed"));
            }
            figures.Add(MarketProse.Moving(quote.Moved(), quote.Hours()));
            text.Children.Add(Widgets.Text(string.Join(" · ", figures), "Caption"));
        }
        grid.Children.Add(text);

        // A chart, or the reason there is not one. A flat line drawn from a
        // single reading is a lie a chart tells very convincingly.
        FrameworkElement chart = quote.History.Count < 2
            ? new Border
            {
                Width = 150,
                Height = 52,
                CornerRadius = new CornerRadius(4),
                Background = (Brush)Application.Current.Resources["ControlFillColorDefaultBrush"],
                Child = new TextBlock { Text = "Not enough\nhistory yet", Style = (Style)Application.Current.Resources["Caption"], TextAlignment = TextAlignment.Center, HorizontalAlignment = HorizontalAlignment.Center, VerticalAlignment = VerticalAlignment.Center, Foreground = Widgets.Tertiary },
            }
            : Sparkline(quote.Prices(), 150, 52);
        Grid.SetColumn(chart, 1);
        grid.Children.Add(chart);

        var right = new StackPanel { Spacing = 5, VerticalAlignment = VerticalAlignment.Center };
        right.Children.Add(new TextBlock { Text = quote.Latest is { } last ? MarketProse.Gold(last.Price) : "—", Style = (Style)Application.Current.Resources["Figure"], FontFamily = Mono, HorizontalAlignment = HorizontalAlignment.Right });
        // Coloured by direction, which is what the series did. It is not
        // advice: a price going up is good news for a seller and bad for a
        // buyer, and nothing here knows which one is reading.
        var change = new TextBlock { Text = "—", Style = (Style)Application.Current.Resources["Caption"], FontFamily = Mono, HorizontalAlignment = HorizontalAlignment.Right };
        if (quote.Change() is { } moved)
        {
            change.Text = string.Create(CultureInfo.InvariantCulture, $"{(moved > 0 ? "+" : "−")}{Math.Abs(moved) * 100:F1}%");
            change.Foreground = new SolidColorBrush(moved > 0 ? Windows.UI.Color.FromArgb(255, 0x6C, 0xCB, 0x5F) : Windows.UI.Color.FromArgb(255, 0xFF, 0x99, 0xA4));
        }
        right.Children.Add(change);
        Grid.SetColumn(right, 2);
        grid.Children.Add(right);

        var drop = new Button { Content = new FontIcon { Glyph = "", FontSize = 14 }, VerticalAlignment = VerticalAlignment.Center, Background = null, BorderThickness = new Thickness(0) };
        ToolTipService.SetToolTip(drop, "Stop watching this item");
        var id = quote.ItemId;
        drop.Click += async (_, _) => await account.UnwatchItem(id);
        Grid.SetColumn(drop, 3);
        grid.Children.Add(drop);

        return new Border { Style = (Style)Application.Current.Resources["RowDivider"], Child = grid };
    }

    /// <summary>The token, the auction houses being fetched, and the horizon.</summary>
    private Border WatchingRail()
    {
        var rail = new StackPanel { Spacing = 12 };
        if (view.TokenPrice is { } token)
        {
            rail.Children.Add(Widgets.CardHead("WoW Token"));
            rail.Children.Add(new TextBlock { Text = MarketProse.Gold(token), Style = (Style)Application.Current.Resources["LargeFigure"], FontFamily = Mono });
            // No chart: the token arrives as one current price with no series behind it.
            rail.Children.Add(Widgets.Text("Region-wide, and the one price Blizzard sets rather than players.", "Caption"));
        }
        rail.Children.Add(Widgets.CardHead("Auction houses"));
        rail.Children.Add(HouseLine("Region-wide commodities", "Always", null));
        foreach (var (id, name) in view.Realms)
        {
            // The date the realm was added is not recorded anywhere, so this
            // is the oldest price still held for it: a floor on the same fact.
            var oldest = view.Quotes.Where(quote => quote.Realm == id).Select(quote => quote.Since).Where(since => since is not null).Min();
            rail.Children.Add(HouseLine(name, oldest is { } at ? $"Prices since {at.ToLocalTime().ToString("d MMM", CultureInfo.InvariantCulture)}" : "No prices yet", id));
        }
        rail.Children.Add(Widgets.Text("History has a thirty-day horizon because the API terms require one. It is a term of the licence rather than a cache policy — so the answer to \"we need more\" is richer readings inside the window, never older ones.", "Caption"));
        rail.Children.Add(Widgets.Text("Sales inferred from stock disappearing between snapshots.", "Caption"));
        return Widgets.Card(rail);
    }

    private Grid HouseLine(string name, string when, long? realm)
    {
        var line = new Grid { ColumnSpacing = 8 };
        line.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        line.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        line.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        line.Children.Add(new TextBlock { Text = name, VerticalAlignment = VerticalAlignment.Center });
        var stamp = new TextBlock { Text = when, Style = (Style)Application.Current.Resources["Caption"], VerticalAlignment = VerticalAlignment.Center };
        Grid.SetColumn(stamp, 1);
        line.Children.Add(stamp);
        if (realm is { } id)
        {
            var drop = new Button { Content = new FontIcon { Glyph = "", FontSize = 12 }, Background = null, BorderThickness = new Thickness(0), Padding = new Thickness(6) };
            ToolTipService.SetToolTip(drop, "Stop fetching this realm");
            drop.Click += async (_, _) => await account.UnwatchRealm(id);
            Grid.SetColumn(drop, 2);
            line.Children.Add(drop);
        }
        return line;
    }

    // -- crafting -------------------------------------------------------------

    /// <summary>What to make, what to sell, and what to buy before it is gone.</summary>
    private void DrawCrafting()
    {
        column.Children.Add(Widgets.Text("Ranked by margin × what is actually selling", "Caption"));
        var split = new Grid { ColumnSpacing = 16 };
        split.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        split.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(312) });
        var main = new StackPanel { Spacing = 16 };

        // First, because it is the only thing on this page that expires.
        if (view.Offers.Count > 0)
        {
            var block = new StackPanel { Spacing = 8 };
            block.Children.Add(Widgets.CardHead("Missing, and for sale"));
            foreach (var offer in view.Offers.Take(ShownOffers))
            {
                var realm = view.Realms.FirstOrDefault(named => named.Id == offer.Realm).Name ?? "the region";
                block.Children.Add(Compact(offer.Kind.Singular(), offer.Name, string.Create(CultureInfo.InvariantCulture, $"{offer.Quantity:N0} listed on {realm}"), MarketProse.Gold(offer.UnitPrice)));
            }
            if (view.Offers.Count > ShownOffers)
            {
                block.Children.Add(Widgets.Text(string.Create(CultureInfo.InvariantCulture, $"and {view.Offers.Count - ShownOffers} more — narrow it down in the collection pages"), "Caption"));
            }
            main.Children.Add(Widgets.Card(block));
        }

        var worth = view.Crafting.Worth;
        var unmeasured = view.Crafting.Unmeasured;
        // The one recipe with the fattest paper margin, when the ranking has
        // put something else above it. That disagreement is the feature.
        var fattest = worth.Take(RankedShown).Select((entry, index) => (entry.Margin, index)).OrderByDescending(pair => pair.Margin).Select(pair => (int?)pair.index).FirstOrDefault();
        if (worth.Count > 0)
        {
            var cards = new StackPanel { Spacing = 10 };
            for (var index = 0; index < Math.Min(worth.Count, RankedShown); index++)
            {
                cards.Children.Add(MakingCard(worth[index], index == 0, fattest == index && index > 0 ? index : null));
            }
            if (worth.Count > RankedShown)
            {
                cards.Children.Add(Widgets.Text(string.Create(CultureInfo.InvariantCulture, $"and {worth.Count - RankedShown} more"), "Caption"));
            }
            main.Children.Add(cards);
        }
        // Said out loud rather than left as a shorter list. A recipe whose
        // reagents have no price is not a bad flip, it is one nobody has priced.
        var short_ = unmeasured.MissingReagent + unmeasured.MissingOutput;
        if (short_ > 0)
        {
            main.Children.Add(Widgets.Standing(
                $"{Chronicle.Prose.Plural(short_, "recipe", "recipes")} could not be priced",
                "Something they need, or the thing they make, has not been seen on a watched realm. They are counted here rather than quietly dropped — watch the realm you craft on and they fill in.",
                InfoBarSeverity.Warning));
        }
        if (worth.Count == 0 && unmeasured == default)
        {
            main.Children.Add(Widgets.Standing("No recipe books yet", "Open each character's profession window once. The game will not tell an addon what somebody can make until they do."));
        }
        if (view.Resale.Count > 0)
        {
            var block = new StackPanel { Spacing = 8 };
            block.Children.Add(Widgets.CardHead("Spares worth selling"));
            foreach (var entry in view.Resale.Take(ShownOffers))
            {
                // The cheapest quality's price, deliberately: Armory knows the
                // quality of every pet listed and of none in your own journal.
                var card = Compact("Pet", entry.Name, $"{Chronicle.Prose.Plural(entry.Spare, "spare", "spares")} · {MarketProse.Moving(entry.Sold, entry.SpanHours)}", MarketProse.Gold(entry.Floor));
                if (entry.Ceiling > entry.Floor)
                {
                    card.Children.Add(new TextBlock { Text = $"up to {MarketProse.Gold(entry.Ceiling)}", Style = (Style)Application.Current.Resources["Caption"], FontFamily = Mono, HorizontalAlignment = HorizontalAlignment.Right });
                }
                block.Children.Add(card);
            }
            if (view.Resale.Count > ShownOffers)
            {
                block.Children.Add(Widgets.Text(string.Create(CultureInfo.InvariantCulture, $"and {view.Resale.Count - ShownOffers} more"), "Caption"));
            }
            main.Children.Add(Widgets.Card(block));
        }
        split.Children.Add(main);
        var rail = CraftingRail();
        Grid.SetColumn(rail, 1);
        split.Children.Add(rail);
        column.Children.Add(split);
    }

    /// <summary>One craft worth making, and who should make it.</summary>
    private static Border MakingCard(Making entry, bool top, int? paper)
    {
        var grid = new Grid { ColumnSpacing = 15 };
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(120) });
        var text = new StackPanel { Spacing = 4 };
        var heading = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 8 };
        heading.Children.Add(new TextBlock { Text = entry.Name, Style = (Style)Application.Current.Resources["BodyStrongTextBlockStyle"], TextTrimming = TextTrimming.CharacterEllipsis });
        if (entry.Makes > 1)
        {
            heading.Children.Add(new TextBlock { Text = string.Create(CultureInfo.InvariantCulture, $"×{entry.Makes}"), Style = (Style)Application.Current.Resources["Caption"], FontFamily = Mono, VerticalAlignment = VerticalAlignment.Bottom });
        }
        text.Children.Add(heading);
        text.Children.Add(Widgets.Text($"{entry.ByName} · {entry.RealmName} · {MarketProse.Gold(entry.Cost)} in reagents · {MarketProse.Moving(entry.Sold, entry.SpanHours)}", "Caption"));
        // Shown, and deliberately not taken off the cost: the addon's Warband
        // bag indices have never been confirmed against a stocked bank, and a
        // wrong index has to look wrong rather than inflate a margin.
        if (entry.Held.Count > 0)
        {
            text.Children.Add(new TextBlock { Text = $"{Chronicle.Prose.Plural(entry.Held.Count, "reagent", "reagents")} in the Warband bank — not taken off the cost", Style = (Style)Application.Current.Resources["Caption"], Foreground = Widgets.AccentBrush, TextWrapping = TextWrapping.Wrap });
        }
        if (paper is { } index)
        {
            text.Children.Add(Widgets.Text($"Big margin, almost no buyers — which is why it is {MarketProse.Ordinal(index).ToLowerInvariant()}", "Caption"));
        }
        grid.Children.Add(text);
        var right = new StackPanel { Spacing = 2, VerticalAlignment = VerticalAlignment.Center };
        right.Children.Add(new TextBlock { Text = $"+{MarketProse.Gold(Math.Max(entry.Margin, 0))}", Style = (Style)Application.Current.Resources["Figure"], FontFamily = Mono, HorizontalAlignment = HorizontalAlignment.Right, Foreground = Widgets.AccentBrush });
        right.Children.Add(new TextBlock { Text = "each, after the cut", Style = (Style)Application.Current.Resources["Caption"], HorizontalAlignment = HorizontalAlignment.Right });
        Grid.SetColumn(right, 1);
        grid.Children.Add(right);
        var card = Widgets.Card(grid);
        if (top)
        {
            card.BorderBrush = Widgets.AccentBrush;
        }
        if (paper is not null)
        {
            card.Opacity = 0.82;
        }
        return card;
    }

    /// <summary>How the ranking works, whose books it read, and what it assumed.</summary>
    private Border CraftingRail()
    {
        var rail = new StackPanel { Spacing = 8 };
        rail.Children.Add(Widgets.CardHead("How this is ranked"));
        rail.Children.Add(Widgets.Text("Margin × what has actually been selling, not margin. A four-hundred-gold profit on something nobody buys is forty unsold flasks.", "Caption"));
        rail.Children.Add(Widgets.Text("Sale volume is inferred from stock disappearing between hourly snapshots. Blizzard records no sale anywhere.", "Caption"));

        rail.Children.Add(new TextBlock { Text = "Recipe books", Style = (Style)Application.Current.Resources["CardHeading"], Margin = new Thickness(0, 8, 0, 0) });
        // Whose books answered. A character with nothing here is silence
        // rather than a character who can make nothing.
        var names = view.Crafting.Worth.GroupBy(entry => entry.ByName).Select(group => (Name: group.Key, Count: group.Count())).OrderByDescending(pair => pair.Count).ThenBy(pair => pair.Name, StringComparer.Ordinal).ToList();
        if (names.Count == 0)
        {
            rail.Children.Add(Widgets.Text("No book has been read yet.", "Caption"));
        }
        foreach (var (name, count) in names)
        {
            var line = new Grid { ColumnSpacing = 9 };
            line.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
            line.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
            line.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
            line.Children.Add(new Ellipse { Width = 7, Height = 7, Fill = Widgets.AccentBrush, VerticalAlignment = VerticalAlignment.Center });
            var label = new TextBlock { Text = name };
            Grid.SetColumn(label, 1);
            line.Children.Add(label);
            var figure = new TextBlock { Text = string.Create(CultureInfo.InvariantCulture, $"{count} worth making"), Style = (Style)Application.Current.Resources["Caption"] };
            Grid.SetColumn(figure, 2);
            line.Children.Add(figure);
            rail.Children.Add(line);
        }
        rail.Children.Add(Widgets.Text("Open each character's profession window once. The game will not tell an addon what somebody can make until you do, and there is no way around it.", "Caption"));

        rail.Children.Add(new TextBlock { Text = "What these figures assume", Style = (Style)Application.Current.Resources["CardHeading"], Margin = new Thickness(0, 8, 0, 0) });
        foreach (var line in new[]
        {
            "A one-star craft, every time. Quality depends on skill, specialisation and luck, and Armory reads none of them.",
            "Reagents at the cheapest quality that has a price, minus the auction house's five percent.",
            "Warband stock is shown beside a row and never subtracted — the bag indices are unconfirmed, and a wrong number you can see beats one folded into a margin.",
        })
        {
            rail.Children.Add(Widgets.Text(line, "Caption"));
        }
        return Widgets.Card(rail);
    }

    /// <summary>A small card: a kind, a name, a note, and a price.</summary>
    private static StackPanel Compact(string kind, string title, string subtitle, string price)
    {
        var card = new StackPanel { Spacing = 2 };
        var line = new Grid { ColumnSpacing = 11 };
        line.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        line.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        var text = new StackPanel { Spacing = 2 };
        text.Children.Add(new TextBlock { Text = title, Style = (Style)Application.Current.Resources["BodyStrongTextBlockStyle"], TextTrimming = TextTrimming.CharacterEllipsis });
        text.Children.Add(Widgets.Text($"{kind} · {subtitle}", "Caption"));
        line.Children.Add(text);
        var figure = new TextBlock { Text = price, Style = (Style)Application.Current.Resources["Figure"], FontFamily = Mono, VerticalAlignment = VerticalAlignment.Center };
        Grid.SetColumn(figure, 1);
        line.Children.Add(figure);
        card.Children.Add(new Border { Style = (Style)Application.Current.Resources["RowDivider"], Child = line });
        return card;
    }

    /// <summary>A price series as a line, the way the momentum chart draws one: the accent, no axes, the span of what is held.</summary>
    internal static Canvas Sparkline(IReadOnlyList<double> values, double width, double height)
    {
        var canvas = new Canvas { Width = width, Height = height };
        if (values.Count < 2)
        {
            return canvas;
        }
        var low = values.Min();
        var high = values.Max();
        var span = Math.Max(high - low, 1e-9);
        var line = new Polyline { Stroke = Widgets.AccentBrush, StrokeThickness = 2, StrokeLineJoin = PenLineJoin.Round };
        for (var i = 0; i < values.Count; i++)
        {
            var x = i * (width - 4) / (values.Count - 1) + 2;
            var y = height - 4 - (values[i] - low) / span * (height - 8);
            line.Points.Add(new Windows.Foundation.Point(x, y));
        }
        canvas.Children.Add(line);
        return canvas;
    }

    private static FontFamily Mono => new("Cascadia Mono, Consolas");
}
