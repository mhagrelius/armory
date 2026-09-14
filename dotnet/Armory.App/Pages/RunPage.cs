using System.Globalization;
using Armory.App.Dialogs;
using Armory.Chronicle;
using Armory.Client.Shell;
using Armory.Run;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Controls.Primitives;
using Microsoft.UI.Xaml.Media;

namespace Armory.App.Pages;

/// <summary>
/// The home page: how far the run has got, what is closest, and what the
/// person must settle by hand. Design option 2a: an InfoBar for the logout
/// caveat, a hero card with the ring, then within reach and the road so far
/// on the left and the attestations on the right, and the goal browser —
/// the GTK page's second navigation page — as a card of its own at the foot.
/// </summary>
public sealed partial class RunPage : Page, ISearchable
{
    /// <summary>
    /// How many goals to list at once. A run over a decade-old account has
    /// thousands of poisoned goals, and a page holding all of them is a page
    /// that takes a second to appear. Anything past the cap is reachable by
    /// searching rather than by finishing something first.
    /// </summary>
    private const int Shown = 150;

    /// <summary>How many entries the road shows before it stops. It is a summary of what has happened, not the journal — the journal is a place of its own.</summary>
    private const int RoadShown = 14;

    /// <summary>How big an achievement's icon is on a goal row.</summary>
    private const int Art = 32;

    /// <summary>
    /// Which set of goals is being looked at. Not <see cref="Bucket"/>, which
    /// is the classification a goal carries: this is a view over those.
    /// <c>Done</c> spans two of them, and <c>Spent</c> covers every kind of
    /// exclusion.
    /// </summary>
    private enum GoalTab
    {
        /// <summary>Poisoned, measurable, and not finished. The work.</summary>
        ToDo,
        /// <summary>Poisoned with nothing able to measure it.</summary>
        Attest,
        /// <summary>Settled, whether by being earned during the run or by being attested.</summary>
        Done,
        /// <summary>Outside the denominator entirely.</summary>
        Spent,
    }

    private static readonly GoalTab[] AllTabs = [GoalTab.ToDo, GoalTab.Attest, GoalTab.Done, GoalTab.Spent];

    private readonly Account account;
    private readonly MainWindow window;
    private readonly StackPanel column = new() { Spacing = 16 };
    private readonly StackPanel browser = new() { Spacing = 12 };
    private Border? browserCard;
    private GoalTab tab = GoalTab.ToDo;
    private string needle = "";

    public RunPage(Account account, MainWindow window)
    {
        this.account = account;
        this.window = window;
        Content = new ScrollViewer { Content = column, Padding = (Thickness)Application.Current.Resources["PagePadding"] };
        // The icons the sync spends its budget on: the goals somebody could
        // still act on, not every goal in the run.
        account.AchievementArtWanted = ArtWanted;
        account.Changed += Redraw;
        Redraw();
    }

    public string SearchPlaceholder => "Search goals";

    public void Search(string text)
    {
        var trimmed = text.Trim();
        if (trimmed == needle)
        {
            return;
        }
        var opening = needle.Length == 0;
        needle = trimmed;
        DrawBrowser();
        // Typing is asking for the list.
        if (opening && browserCard is not null)
        {
            browserCard.StartBringIntoView();
        }
    }

    private async void Redraw()
    {
        // The chronicle's, read by the run: the fortnight strip, last night's
        // numbers and the evenings on the road all come from the sessions.
        var sessions = await account.Sessions();
        column.Children.Clear();
        // The browser panel outlives its card: clearing the column detaches
        // the card, not the panel from it, and a panel that still has a
        // parent cannot be given to the next card.
        if (browserCard is not null)
        {
            browserCard.Child = null;
            browserCard = null;
        }
        if (account.Run is not { } held)
        {
            DrawNoRun();
            return;
        }
        var run = held.Run;
        var progress = run.Progress();
        var now = DateTimeOffset.UtcNow;
        var day = Math.Max((long)(now - run.Baseline.TakenAt).TotalDays, 0) + 1;

        var sync = Widgets.Accent("Sync", "");
        sync.Click += async (_, _) => await account.ShareNow();
        var all = Widgets.Standard(string.Create(CultureInfo.InvariantCulture, $"All {Thousands(run.Goals.Count)} goals"));
        all.Click += (_, _) => ShowTab(GoalTab.ToDo);
        var more = new Button { Content = new FontIcon { Glyph = "", FontSize = 16 }, MinHeight = 34 };
        more.Flyout = new MenuFlyout
        {
            Items =
            {
                Command("Start a new run…", async () => await ConfirmNewRun()),
                Command("Account & Sharing…", async () => await window.ShowSharing()),
            },
        };
        column.Children.Add(Widgets.Header(
            "Run",
            string.Create(CultureInfo.InvariantCulture, $"Day {day} of {run.Name} · {Thousands(progress.Excluded)} goals this account has already spent, left out of the count"),
            sync, all, more));

        var lastSynced = account.CollectedAt is { } at ? Account.When(at, now) : "never";
        column.Children.Add(Widgets.Standing(
            $"Last synced {lastSynced}",
            "World of Warcraft writes its saved variables at logout, so tonight arrives once you log out."));

        column.Children.Add(Widgets.Card(Hero(run, progress, sessions)));
        if (LastNight(sessions) is { } lastNight)
        {
            column.Children.Add(lastNight);
        }

        var grid = new Grid { ColumnSpacing = 16 };
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1.15, GridUnitType.Star) });
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        var left = new StackPanel { Spacing = 16 };
        left.Children.Add(Widgets.Card(WithinReach(run)));
        left.Children.Add(Widgets.Card(RoadSoFar(run, progress, sessions)));
        grid.Children.Add(left);
        var right = Widgets.Card(OnlyYou(run));
        Grid.SetColumn(right, 1);
        grid.Children.Add(right);
        column.Children.Add(grid);

        browserCard = Widgets.Card(browser);
        column.Children.Add(browserCard);
        DrawBrowser();
    }

    private void DrawNoRun()
    {
        var enrolled = account.Cohort.Count;
        var start = Widgets.Accent("Start a run");
        start.Click += async (_, _) => await account.StartRun();
        column.Children.Add(Widgets.Header("Run", "Nothing is being measured yet", start));
        var text = enrolled == 0
            ? "A run is a replay of content the account already remembers, measured per enrolled character rather than account-wide. Enrol at least one character on Roster, then start one."
            : string.Create(CultureInfo.InvariantCulture, $"{enrolled} enrolled. Starting a run freezes what the account has now and measures everything from then on.");
        column.Children.Add(Widgets.Card(Widgets.Text(text, "Secondary")));
    }

    private static UIElement Hero(Run.Run run, Progress progress, List<Session> sessions)
    {
        var grid = new Grid { ColumnSpacing = 24 };
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });

        var ring = new Grid { Width = 116, Height = 116 };
        ring.Children.Add(new ProgressRing { Width = 116, Height = 116, IsIndeterminate = false, Minimum = 0, Maximum = 1, Value = progress.Fraction, Background = (Brush)Application.Current.Resources["ControlFillColorDefaultBrush"] });
        var inner = new StackPanel { HorizontalAlignment = HorizontalAlignment.Center, VerticalAlignment = VerticalAlignment.Center };
        inner.Children.Add(new TextBlock { Text = string.Create(CultureInfo.InvariantCulture, $"{Math.Round(progress.Fraction * 100)}%"), Style = (Style)Application.Current.Resources["LargeFigure"], HorizontalAlignment = HorizontalAlignment.Center });
        inner.Children.Add(new TextBlock { Text = string.Create(CultureInfo.InvariantCulture, $"{Thousands(progress.Done)} of {Thousands(progress.Counted)}"), Style = (Style)Application.Current.Resources["Caption"], HorizontalAlignment = HorizontalAlignment.Center });
        ring.Children.Add(inner);
        grid.Children.Add(ring);

        var week = run.Goals.Count(goal => goal.Counts && goal.IsDone && DoneAt(goal) is { } at && at > DateTimeOffset.UtcNow.AddDays(-7));
        var beside = new StackPanel { Spacing = 8, VerticalAlignment = VerticalAlignment.Center };
        beside.Children.Add(new TextBlock { Text = Closed(week), Style = (Style)Application.Current.Resources["Subtitle"] });
        beside.Children.Add(new TextBlock
        {
            Text = string.Create(CultureInfo.InvariantCulture, $"{Thousands(progress.AwaitingAttestation)} waiting on your word · {Thousands(progress.Excluded)} cannot be earned again"),
            Style = (Style)Application.Current.Resources["Caption"],
        });
        // What it has been like lately: the last fourteen days, a bar a
        // day, and no bar at all on a day nobody played.
        beside.Children.Add(ChronicleWidgets.Momentum(Cards.Fortnight(sessions, DateOnly.FromDateTime(DateTime.UtcNow))));
        var captions = new Grid();
        captions.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        captions.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        captions.Children.Add(new TextBlock { Text = "Two weeks ago", Style = (Style)Application.Current.Resources["Caption"] });
        var last = new TextBlock { Text = "Last night", Style = (Style)Application.Current.Resources["Caption"] };
        Grid.SetColumn(last, 1);
        captions.Children.Add(last);
        beside.Children.Add(captions);
        Grid.SetColumn(beside, 1);
        grid.Children.Add(beside);
        return grid;
    }

    /// <summary>
    /// Last night's numbers: the chronicle's, at the top of the page rather
    /// than buried in the evening they came from. Hidden entirely when there
    /// was no last night; an empty row of dashes is not a lighter version of
    /// this block. Pressing one goes and reads about it.
    /// </summary>
    private UIElement? LastNight(List<Session> sessions)
    {
        var session = sessions.MaxBy(session => session.StartedAt);
        if (session is null)
        {
            return null;
        }
        var digest = session.Digest();
        var minutes = Math.Max((long)digest.Duration.TotalMinutes, 0);
        var block = new StackPanel { Spacing = 8 };
        var spent = Cards.SpentIn(digest) ?? digest.DisplayName;
        block.Children.Add(ChronicleWidgets.Meta(string.Create(CultureInfo.InvariantCulture, $"Last night — {spent}, {minutes / 60}h {minutes % 60}m")));
        var cards = new Grid { ColumnSpacing = 12 };
        var figures = new List<(string Caption, string Figure, string? Note)>
        {
            ("Where", spent, Cards.MetaLine(digest, TimeZoneInfo.Local.GetUtcOffset(digest.StartedAt))),
        };
        if (digest.Quests.Count > 0)
        {
            figures.Add(("Quests turned in", Thousands(digest.Quests.Count), digest.Quests[0].Title));
        }
        // Three numbers there used to be. The hardest hit taken and the
        // closest call went with the combat log in patch 12.0; what is left
        // is the one the game still tells us.
        if (digest.LongestFight > 0)
        {
            figures.Add(("Longest fight", Cards.FightLength(digest.LongestFight), null));
        }
        if (digest.Purse != 0)
        {
            figures.Add(("Purse", Prose.Purse(digest.Purse), null));
        }
        for (var i = 0; i < figures.Count; i++)
        {
            cards.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
            var card = Widgets.Stat(figures[i].Caption, figures[i].Figure, figures[i].Note);
            card.PointerPressed += (_, _) => window.Open("chronicle");
            Grid.SetColumn(card, i);
            cards.Children.Add(card);
        }
        block.Children.Add(cards);
        return block;
    }

    private static string Closed(int week) => week switch
    {
        0 => "Nothing closed this week",
        1 => "One closed this week",
        2 => "Two closed this week",
        3 => "Three closed this week",
        _ => string.Create(CultureInfo.InvariantCulture, $"{week} closed this week"),
    };

    private static DateTimeOffset? DoneAt(Goal goal) => goal.Standing switch
    {
        Standing.EarnedDuringRun earned => earned.At,
        _ => goal.Attestation?.At,
    };

    /// <summary>
    /// The three nearest to closing. Sorted by what is left rather than by
    /// percentage, because what somebody wants from this list is something
    /// they can finish tonight. Pressing one opens it.
    /// </summary>
    private UIElement WithinReach(Run.Run run)
    {
        var stack = new StackPanel();
        stack.Children.Add(Widgets.CardHead("Within reach", "nearest first"));
        var (nearest, _) = RowsFor(GoalTab.ToDo, run, "");
        if (nearest.Count == 0)
        {
            stack.Children.Add(Widgets.Text("Nothing measurable is close. Every open goal is either waiting on your word or too far to say.", "Secondary"));
            return stack;
        }
        var light = Widgets.IsLight(this);
        foreach (var goal in nearest.Take(3))
        {
            var evaluation = goal.Evaluation!.Value;
            var row = new Grid { ColumnSpacing = 12, Margin = new Thickness(0, 6, 0, 6) };
            row.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
            row.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
            row.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
            row.Children.Add(Icon(goal.AchievementId));
            var middle = new StackPanel { Spacing = 4, VerticalAlignment = VerticalAlignment.Center };
            middle.Children.Add(new TextBlock { Text = Name(goal.AchievementId), Style = (Style)Application.Current.Resources["BodyStrongTextBlockStyle"] });
            middle.Children.Add(Widgets.Bar(evaluation.Fraction));
            Grid.SetColumn(middle, 1);
            row.Children.Add(middle);
            // Whichever enrolled character the figure was measured against.
            // The number means nothing without them: "eleven to go" is a
            // fact about somebody in particular, not about the account.
            var who = goal.Nearest is { } key ? account.Roster.Get(key) : null;
            var end = new StackPanel { HorizontalAlignment = HorizontalAlignment.Right };
            end.Children.Add(new TextBlock { Text = string.Create(CultureInfo.InvariantCulture, $"{Thousands(Remaining(goal))} to go"), Style = (Style)Application.Current.Resources["BodyStrongTextBlockStyle"], HorizontalAlignment = HorizontalAlignment.Right });
            end.Children.Add(Widgets.Named(who, goal.Nearest?.DisplayName() ?? "", light));
            Grid.SetColumn(end, 2);
            row.Children.Add(end);
            OpenOnTap(row, goal);
            stack.Children.Add(row);
        }
        return stack;
    }

    /// <summary>
    /// What has actually happened, newest first: a merge of the evenings
    /// played, the goals closed and the day the run began, sorted together
    /// rather than grouped, because "the night I enrolled Aeltor" is the same
    /// kind of fact as "the night I closed Loremaster".
    /// </summary>
    private UIElement RoadSoFar(Run.Run run, Progress progress, List<Session> sessions)
    {
        var stack = new StackPanel();
        var head = Widgets.CardHead("The road so far");
        var chronicle = new HyperlinkButton { Content = "Chronicle", Padding = new Thickness(4, 0, 4, 0) };
        chronicle.Click += (_, _) => window.Open("chronicle");
        Grid.SetColumn(chronicle, 1);
        head.Children.Add(chronicle);
        stack.Children.Add(head);
        var began = (
            At: run.Baseline.TakenAt,
            Title: string.Create(CultureInfo.InvariantCulture, $"{run.Name} began"),
            Detail: string.Create(CultureInfo.InvariantCulture, $"A snapshot of what the account already had — {Thousands(progress.Excluded)} goals left out of the count"));
        var entries = run.Goals
            .Where(goal => goal.Counts && goal.IsDone && DoneAt(goal) is not null)
            .Select(goal => (At: DoneAt(goal)!.Value, Title: Name(goal.AchievementId), Detail: goal.Attestation is not null ? "Settled on your word" : "Earned during the run"))
            .Concat(sessions
                .Where(session => session.StartedAt >= run.Baseline.TakenAt)
                .Select(session => session.Digest())
                .Select(digest => (At: digest.StartedAt, Cards.RoadLine(digest).Title, Cards.RoadLine(digest).Detail)))
            .Append(began)
            .OrderByDescending(entry => entry.At)
            .Take(RoadShown)
            .ToList();
        var first = true;
        foreach (var (at, title, detail) in entries)
        {
            var row = new Grid { ColumnSpacing = 12, Margin = new Thickness(0, 6, 0, 6) };
            row.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
            row.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
            row.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
            row.Children.Add(new Microsoft.UI.Xaml.Shapes.Ellipse
            {
                Width = 8,
                Height = 8,
                VerticalAlignment = VerticalAlignment.Center,
                Fill = first ? Widgets.AccentBrush : new SolidColorBrush(Windows.UI.Color.FromArgb(92, 255, 255, 255)),
            });
            var middle = new StackPanel();
            middle.Children.Add(new TextBlock { Text = title, Style = (Style)Application.Current.Resources["BodyStrongTextBlockStyle"], TextWrapping = TextWrapping.Wrap });
            middle.Children.Add(new TextBlock { Text = detail, Style = (Style)Application.Current.Resources["Caption"], TextWrapping = TextWrapping.Wrap });
            Grid.SetColumn(middle, 1);
            row.Children.Add(middle);
            var when = new TextBlock { Text = at.ToLocalTime().ToString("d MMM", CultureInfo.InvariantCulture), Style = (Style)Application.Current.Resources["Caption"], VerticalAlignment = VerticalAlignment.Center };
            Grid.SetColumn(when, 2);
            row.Children.Add(when);
            stack.Children.Add(row);
            first = false;
        }
        return stack;
    }

    /// <summary>The count is the point. Five switches is what fits; the rest are a press away, on the browser's second tab.</summary>
    private UIElement OnlyYou(Run.Run run)
    {
        var stack = new StackPanel();
        var (waiting, settle) = RowsFor(GoalTab.Attest, run, "");
        stack.Children.Add(Widgets.CardHead("Only you can settle", string.Create(CultureInfo.InvariantCulture, $"{Thousands(settle)} waiting")));
        // The short form of the tab's description. The card is a few
        // switches high and the full paragraph pushes them off the page; the
        // whole argument is still on the tab the link opens.
        stack.Children.Add(Widgets.Text("The game keeps no per-character record of these. Tick the ones you have done again.", "Secondary"));
        if (settle == 0)
        {
            stack.Children.Add(Widgets.Text("Nothing is waiting on you.", "Caption"));
            return stack;
        }
        foreach (var goal in waiting.Take(5))
        {
            var row = new Grid { ColumnSpacing = 12, Margin = new Thickness(0, 8, 0, 0) };
            row.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
            row.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
            var text = new StackPanel();
            text.Children.Add(new TextBlock { Text = Name(goal.AchievementId), Style = (Style)Application.Current.Resources["BodyStrongTextBlockStyle"] });
            text.Children.Add(new TextBlock { Text = Subtitle(goal.AchievementId, ""), Style = (Style)Application.Current.Resources["Caption"] });
            row.Children.Add(text);
            var toggle = AttestSwitch(goal);
            Grid.SetColumn(toggle, 1);
            row.Children.Add(toggle);
            stack.Children.Add(row);
        }
        var link = new HyperlinkButton { Content = string.Create(CultureInfo.InvariantCulture, $"All {Thousands(settle)} to settle"), Margin = new Thickness(0, 8, 0, 0), Padding = new Thickness(0) };
        link.Click += (_, _) => ShowTab(GoalTab.Attest);
        stack.Children.Add(link);
        return stack;
    }

    // -- the goal browser -----------------------------------------------------

    /// <summary>Open one tab and put it on screen. How the header's "All … goals" and the settle link get somebody to the list.</summary>
    private void ShowTab(GoalTab wanted)
    {
        if (wanted != tab)
        {
            tab = wanted;
            DrawBrowser();
        }
        browserCard?.StartBringIntoView();
    }

    /// <summary>
    /// The browser card: four tabs with their counts, and the rows of
    /// whichever one is open. Only the open tab is built — building all four
    /// is six hundred rows and six hundred icons for the three nobody is
    /// looking at, every time anything changes. Which tab is open survives a
    /// redraw: ticking something off re-plans the whole run, and being thrown
    /// back to the first tab every time would make working through a list
    /// impossible.
    /// </summary>
    private void DrawBrowser()
    {
        browser.Children.Clear();
        if (account.Run is not { } held)
        {
            return;
        }
        var run = held.Run;
        browser.Children.Add(Widgets.CardHead("Goals"));

        var selector = new SelectorBar();
        foreach (var candidate in AllTabs)
        {
            // The count is of everything in the tab, not of what answers the
            // search: it has to say how much work there is, and a count that
            // changed as somebody typed would be reporting the search rather
            // than the run.
            var (_, total) = RowsFor(candidate, run, "");
            selector.Items.Add(new SelectorBarItem
            {
                Text = string.Create(CultureInfo.InvariantCulture, $"{Label(candidate)}  {Thousands(total)}"),
                Tag = candidate,
                IsSelected = candidate == tab,
            });
        }
        selector.SelectionChanged += (sender, _) =>
        {
            // Only on a real change, and never inline: a selection event can
            // fire during the first measure pass, and rebuilding the tree
            // there takes the process down.
            var chosen = sender.SelectedItem?.Tag is GoalTab picked ? picked : GoalTab.ToDo;
            if (chosen == tab)
            {
                return;
            }
            tab = chosen;
            DispatcherQueue.TryEnqueue(DrawBrowser);
        };
        browser.Children.Add(selector);

        var (rows, _) = RowsFor(tab, run, needle);
        if (rows.Count == 0)
        {
            var empty = new StackPanel { Spacing = 4, Margin = new Thickness(0, 12, 0, 12) };
            empty.Children.Add(Widgets.Text(needle.Length == 0 ? EmptyTitle(tab) : "No matches", "Subtitle"));
            empty.Children.Add(Widgets.Text(needle.Length == 0 ? Description(tab) : "No goal in this list matches that.", "Secondary"));
            browser.Children.Add(empty);
            return;
        }

        browser.Children.Add(Widgets.Text(Description(tab), "Secondary"));
        var list = new StackPanel();
        foreach (var goal in rows.Take(Shown))
        {
            list.Children.Add(GoalRow(tab, goal));
        }
        if (rows.Count > Shown)
        {
            var more = new StackPanel { Opacity = 0.6, Padding = new Thickness(0, 10, 0, 0) };
            more.Children.Add(new TextBlock { Text = string.Create(CultureInfo.InvariantCulture, $"and {Thousands(rows.Count - Shown)} more"), Style = (Style)Application.Current.Resources["BodyStrongTextBlockStyle"] });
            more.Children.Add(new TextBlock { Text = "Search to reach them", Style = (Style)Application.Current.Resources["Caption"] });
            list.Children.Add(more);
        }
        browser.Children.Add(list);
    }

    private static string Label(GoalTab tab) => tab switch
    {
        GoalTab.ToDo => "To do",
        GoalTab.Attest => "Your word",
        GoalTab.Done => "Done",
        _ => "Spent",
    };

    /// <summary>What the list is, said once, where somebody reading it can see it.</summary>
    private static string Description(GoalTab tab) => tab switch
    {
        GoalTab.ToDo => "Measured from each enrolled character's own progress, nearest first — not from whether the account has the achievement.",
        GoalTab.Attest => "Your account earned these before the run began, and the game keeps no per-character record of them, so nothing can measure whether you have done them again. Tick the ones you have.",
        GoalTab.Done => "Finished by an enrolled character since the run began, or marked done by hand.",
        _ => "Already collected on this account and impossible to earn again — many bind-on-pickup mounts will not drop for an account that has them. These are left out of the count rather than sitting in it as zeroes.",
    };

    private static string EmptyTitle(GoalTab tab) => tab switch
    {
        GoalTab.ToDo => "Nothing measurable left",
        GoalTab.Attest => "Nothing waiting on you",
        GoalTab.Done => "Nothing finished yet",
        _ => "Nothing spent",
    };

    /// <summary>Whether a goal belongs in this view.</summary>
    private static bool Holds(GoalTab tab, Goal goal) => tab switch
    {
        GoalTab.ToDo => goal.Standing.IsPoisoned
            && goal.Bucket is Bucket.Observable
            && !goal.IsDone
            && goal.Evaluation is { Observable: true, Inherited: false },
        GoalTab.Attest => goal.Standing.IsPoisoned
            && goal.Bucket is Bucket.Attestable
            && goal.Attestation is null,
        GoalTab.Done => goal.IsDone || !goal.Standing.IsPoisoned,
        _ => goal.Bucket is Bucket.Excluded,
    };

    /// <summary>How much is left, for the ordering and the lead. Unmeasured sorts last.</summary>
    private static long Remaining(Goal goal) => goal.Evaluation is { } evaluation
        ? Math.Max(evaluation.Required - evaluation.Progress, 0)
        : long.MaxValue;

    /// <summary>
    /// The goals in one tab that answer the search, and how many there are
    /// before the search. To do is ordered by how much is left rather than
    /// how far along, so a goal needing one more quest outranks one 90%
    /// through something enormous; every other tab is by name.
    /// </summary>
    private (List<Goal> Rows, int Total) RowsFor(GoalTab tab, Run.Run run, string needle)
    {
        var held = run.Goals.Where(goal => Holds(tab, goal)).ToList();
        var total = held.Count;
        if (needle.Length > 0)
        {
            held = held
                .Where(goal => Name(goal.AchievementId).Contains(needle, StringComparison.OrdinalIgnoreCase)
                    || Category(goal.AchievementId).Contains(needle, StringComparison.OrdinalIgnoreCase))
                .ToList();
        }
        var ordered = tab == GoalTab.ToDo
            ? held.OrderBy(Remaining).ThenBy(goal => goal.AchievementId)
            : held.OrderBy(goal => Name(goal.AchievementId), StringComparer.OrdinalIgnoreCase).ThenBy(goal => goal.AchievementId);
        return (ordered.ToList(), total);
    }

    /// <summary>One goal, dressed for the list it is in.</summary>
    private UIElement GoalRow(GoalTab tab, Goal goal)
    {
        var grid = new Grid { ColumnSpacing = 12 };
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        grid.Children.Add(Icon(goal.AchievementId));

        var lead = tab switch
        {
            GoalTab.ToDo => string.Create(CultureInfo.InvariantCulture, $"{Thousands(Remaining(goal))} to go"),
            GoalTab.Done => goal.Attestation is { } attestation
                ? string.Create(CultureInfo.InvariantCulture, $"Marked done on {attestation.At.ToLocalTime():d MMMM yyyy}")
                : "Earned during this run",
            _ => "",
        };
        var text = new StackPanel { Spacing = 2, VerticalAlignment = VerticalAlignment.Center };
        text.Children.Add(new TextBlock { Text = Name(goal.AchievementId), Style = (Style)Application.Current.Resources["BodyStrongTextBlockStyle"], TextWrapping = TextWrapping.Wrap });
        var subtitle = Subtitle(goal.AchievementId, lead);
        if (subtitle.Length > 0)
        {
            text.Children.Add(new TextBlock { Text = subtitle, Style = (Style)Application.Current.Resources["Caption"], TextWrapping = TextWrapping.Wrap });
        }
        Grid.SetColumn(text, 1);
        grid.Children.Add(text);

        var suffix = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 8, VerticalAlignment = VerticalAlignment.Center };
        switch (tab)
        {
            case GoalTab.ToDo:
                if (goal.Fraction() is { } fraction)
                {
                    var bar = Widgets.Bar(fraction);
                    bar.Width = 120;
                    bar.VerticalAlignment = VerticalAlignment.Center;
                    suffix.Children.Add(bar);
                }
                suffix.Children.Add(DropButton(goal.AchievementId));
                break;
            case GoalTab.Attest:
                // Only the attestable list gets switches: a switch is for
                // something the person decides, and every other tab is
                // something measured.
                suffix.Children.Add(AttestSwitch(goal));
                suffix.Children.Add(DropButton(goal.AchievementId));
                break;
            case GoalTab.Done:
                suffix.Children.Add(new FontIcon { Glyph = "", FontSize = 14, Foreground = Widgets.AccentBrush });
                break;
            default:
                grid.Opacity = 0.6;
                // Putting one back is the only action that makes sense
                // here, and it is the exact inverse of the button that
                // removed it.
                var restore = Flat("", "Put this back into the run");
                var id = goal.AchievementId;
                restore.Click += async (_, _) => await account.SetExcluded(id, false);
                suffix.Children.Add(restore);
                break;
        }
        Grid.SetColumn(suffix, 2);
        grid.Children.Add(suffix);

        if (tab != GoalTab.Attest)
        {
            OpenOnTap(grid, goal);
        }
        return new Border { Style = (Style)Application.Current.Resources["RowDivider"], Child = grid };
    }

    private ToggleSwitch AttestSwitch(Goal goal)
    {
        var toggle = new ToggleSwitch { IsOn = goal.Attestation is not null, OnContent = "", OffContent = "", MinWidth = 0, VerticalAlignment = VerticalAlignment.Center };
        var id = goal.AchievementId;
        toggle.Toggled += async (sender, _) =>
        {
            // The cohort's first member stands in for "me" until goals
            // carry a character picker.
            var who = ((ToggleSwitch)sender).IsOn ? account.Cohort.Keys.FirstOrDefault() : null;
            await account.Attest(id, who);
        };
        return toggle;
    }

    /// <summary>Take a goal out of the run.</summary>
    private Button DropButton(long achievementId)
    {
        var button = Flat("", "Leave this out of the run");
        button.Click += async (_, _) => await account.SetExcluded(achievementId, true);
        return button;
    }

    private static Button Flat(string glyph, string tooltip)
    {
        var button = new Button
        {
            Content = new FontIcon { Glyph = glyph, FontSize = 14 },
            Background = new SolidColorBrush(Microsoft.UI.Colors.Transparent),
            BorderThickness = new Thickness(0),
            Padding = new Thickness(8, 6, 8, 6),
            VerticalAlignment = VerticalAlignment.Center,
        };
        ToolTipService.SetToolTip(button, tooltip);
        return button;
    }

    /// <summary>Open the detail view when the row is pressed — anywhere on it that is not one of its own controls.</summary>
    private void OpenOnTap(Grid row, Goal goal)
    {
        var achievement = account.Inputs.Catalogue.GetValueOrDefault(goal.AchievementId);
        // A transparent background, so a press on the gap between two
        // children still lands on the row rather than falling through it.
        row.Background = new SolidColorBrush(Microsoft.UI.Colors.Transparent);
        row.Tapped += async (_, args) =>
        {
            if (WithinControl(args.OriginalSource as DependencyObject, row))
            {
                return;
            }
            await AchievementDialog.Show(XamlRoot, account, goal, achievement);
        };
    }

    /// <summary>Whether a press landed on a button or switch inside the row, whose own handler has it.</summary>
    private static bool WithinControl(DependencyObject? source, Grid row)
    {
        for (var node = source; node is not null && node != row; node = VisualTreeHelper.GetParent(node))
        {
            if (node is ButtonBase or ToggleSwitch)
            {
                return true;
            }
        }
        return false;
    }

    /// <summary>One goal's icon, or the placeholder that stands in for it. The picture arrives when the cache has it.</summary>
    private Border Icon(long achievementId)
    {
        var box = Widgets.ArtPlaceholder(small: true);
        box.VerticalAlignment = VerticalAlignment.Center;
        if (account.AchievementArt.TryGetValue(achievementId, out var url))
        {
            _ = Illustrate(box, url);
        }
        return box;
    }

    private async Task Illustrate(Border box, string url)
    {
        if (await ArtLoader.Decode(account.Art, url, Art) is { } picture)
        {
            box.Child = new Image { Source = picture, Stretch = Stretch.UniformToFill };
        }
    }

    /// <summary>
    /// Which goals still have no icon: the ones somebody could act on, not
    /// every goal in the run. A run over a decade-old account has thousands,
    /// and this is one request each.
    /// </summary>
    private IReadOnlyList<long> ArtWanted(int budget) => account.Run is { } held
        ? held.Run.Goals
            .Where(goal => goal.Standing.IsPoisoned && !goal.IsDone)
            .Where(goal => goal.Bucket is not Bucket.Excluded { Why: Exclusion.ByHand })
            .Select(goal => goal.AchievementId)
            .Where(id => !account.AchievementArt.ContainsKey(id))
            .Take(budget)
            .ToList()
        : [];

    private async Task ConfirmNewRun()
    {
        if (account.Run is not { } held)
        {
            await account.StartRun();
            return;
        }
        var run = held.Run;
        var attested = run.Goals.Count(goal => goal.Attestation is not null);
        var enrolled = account.Cohort.Count;
        var detail = string.Create(CultureInfo.InvariantCulture, $"“{run.Name}” was measured from {run.Baseline.TakenAt.ToLocalTime():d MMMM yyyy}. Starting over throws away its baseline and everything planned against it");
        if (attested > 0)
        {
            detail += string.Create(CultureInfo.InvariantCulture, $", including {attested} goal{(attested == 1 ? "" : "s")} you attested to by hand");
        }
        detail += string.Create(CultureInfo.InvariantCulture, $". The new run will be about the {enrolled} character{(enrolled == 1 ? "" : "s")} enrolled on Roster, and measured from now.");
        var dialog = new ContentDialog
        {
            Title = "Start a new run?",
            Content = new TextBlock { Text = detail, TextWrapping = TextWrapping.Wrap },
            PrimaryButtonText = "Start Over",
            CloseButtonText = "Cancel",
            DefaultButton = ContentDialogButton.Close,
            XamlRoot = XamlRoot,
        };
        if (await dialog.ShowAsync() == ContentDialogResult.Primary)
        {
            await account.StartOver();
        }
    }

    /// <summary>
    /// An achievement's name, or its id when the catalogue has not synced.
    /// Showing the id is ugly and honest; showing nothing would hide a row
    /// that is otherwise perfectly actionable.
    /// </summary>
    private new string Name(long id) => account.Inputs.Catalogue.TryGetValue(id, out var achievement) && achievement.Name.Length > 0
        ? achievement.Name
        : string.Create(CultureInfo.InvariantCulture, $"Achievement {id}");

    private string Category(long id) => account.Inputs.Catalogue.TryGetValue(id, out var achievement) ? achievement.Category : "";

    /// <summary>The category and points, with whatever the caller wants said first.</summary>
    private string Subtitle(long id, string lead)
    {
        var parts = new List<string>();
        if (lead.Length > 0)
        {
            parts.Add(lead);
        }
        if (account.Inputs.Catalogue.TryGetValue(id, out var achievement))
        {
            if (achievement.Category.Length > 0)
            {
                parts.Add(achievement.Category);
            }
            if (achievement.Points > 0)
            {
                parts.Add(string.Create(CultureInfo.InvariantCulture, $"{achievement.Points} points"));
            }
        }
        return string.Join("  ·  ", parts);
    }

    private static MenuFlyoutItem Command(string text, Func<Task> action)
    {
        var item = new MenuFlyoutItem { Text = text };
        item.Click += async (_, _) => await action();
        return item;
    }

    internal static string Thousands(long number) => number.ToString("N0", CultureInfo.InvariantCulture);
}
