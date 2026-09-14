using System.Globalization;
using Armory.App.Pages;
using Armory.Blizzard;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;

namespace Armory.App.Dialogs;

/// <summary>
/// Choosing what to watch: a realm out of the region's index with the
/// account's own first, or an item out of Blizzard's catalogue by name.
/// The port of <c>ui/watch_dialog.rs</c>.
/// </summary>
public static class WatchDialog
{
    /// <summary>
    /// Pick a realm to fetch auctions from. The list is held, so the search
    /// box filters it rather than asking Blizzard anything; the account's
    /// own realms go to the top because with thirty-one characters across
    /// nine realms, the one somebody wants is nearly always one of theirs.
    /// </summary>
    public static async Task Realms(XamlRoot root, IReadOnlyList<Realm> realms, IReadOnlyList<string> mine, Func<Realm, Task> chosen)
    {
        var ordered = realms
            .Select(realm => (Other: !mine.Contains(realm.Slug), Realm: realm))
            .OrderBy(entry => entry.Other)
            .ThenBy(entry => entry.Realm.Name, StringComparer.OrdinalIgnoreCase)
            .ToList();

        var list = new ListView { SelectionMode = ListViewSelectionMode.None, IsItemClickEnabled = true, MaxHeight = 420 };
        var rows = ordered.Select(entry => Row(entry.Realm.Name, entry.Other ? "" : "You have a character here", entry.Realm)).ToList();
        foreach (var row in rows)
        {
            list.Items.Add(row);
        }
        var search = new TextBox { PlaceholderText = "Search realms" };
        search.TextChanged += (_, _) =>
        {
            var needle = search.Text.Trim();
            foreach (var row in rows)
            {
                row.Visibility = needle.Length == 0 || ((Realm)row.Tag).Name.Contains(needle, StringComparison.OrdinalIgnoreCase) ? Visibility.Visible : Visibility.Collapsed;
            }
        };

        var column = new StackPanel { Spacing = 10 };
        column.Children.Add(search);
        if (rows.Count == 0)
        {
            column.Children.Add(Widgets.Standing("No realms yet", "Armory has not fetched the realm list. Sync once — it is a single call and the answer only changes when Blizzard opens or merges a realm."));
        }
        column.Children.Add(list);

        var dialog = new ContentDialog { Title = "Add a Realm", Content = column, CloseButtonText = "Cancel", XamlRoot = root };
        list.ItemClick += async (_, args) =>
        {
            if (args.ClickedItem is ListViewItem { Tag: Realm realm })
            {
                dialog.Hide();
                await chosen(realm);
            }
        };
        await dialog.ShowAsync();
    }

    /// <summary>
    /// Search for an item to watch. The rows arrive as a search lands
    /// rather than being filtered from a list held here: there are two
    /// hundred thousand items and no endpoint that lists them. On submit
    /// rather than on every keystroke, because this is a request to
    /// Blizzard and one per character typed would be a dozen for one word.
    /// </summary>
    public static async Task Items(XamlRoot root, Func<string, Task<List<(long Id, string Name)>>> search, Func<long, string, Task> chosen)
    {
        var box = new AutoSuggestBox { PlaceholderText = "Search items by name", QueryIcon = new SymbolIcon(Symbol.Find) };
        var list = new ListView { SelectionMode = ListViewSelectionMode.None, IsItemClickEnabled = true, MaxHeight = 420 };
        var standing = Widgets.Standing("Search for an item", "Type a name to look it up in Blizzard's catalogue. Commodities are priced region-wide; anything else is priced on the realms you have added.");
        var busy = new ProgressRing { IsActive = false, Width = 32, Height = 32, HorizontalAlignment = HorizontalAlignment.Center, Visibility = Visibility.Collapsed };

        var column = new StackPanel { Spacing = 10 };
        column.Children.Add(box);
        column.Children.Add(standing);
        column.Children.Add(busy);
        column.Children.Add(list);

        var dialog = new ContentDialog { Title = "Watch an Item", Content = column, CloseButtonText = "Cancel", XamlRoot = root };
        box.QuerySubmitted += async (sender, _) =>
        {
            var text = sender.Text.Trim();
            if (text.Length < 2)
            {
                return;
            }
            busy.IsActive = true;
            busy.Visibility = Visibility.Visible;
            standing.IsOpen = false;
            var found = await search(text);
            busy.IsActive = false;
            busy.Visibility = Visibility.Collapsed;
            list.Items.Clear();
            foreach (var (id, name) in found)
            {
                list.Items.Add(Row(name, string.Create(CultureInfo.InvariantCulture, $"Item {id}"), (id, name)));
            }
            if (found.Count == 0)
            {
                standing.Title = "Nothing found";
                standing.Message = "No item in Blizzard's catalogue matches that. Names are matched from the start, so a fragment from the middle will not find one.";
                standing.IsOpen = true;
            }
        };
        list.ItemClick += async (_, args) =>
        {
            if (args.ClickedItem is ListViewItem { Tag: (long id, string name) })
            {
                dialog.Hide();
                await chosen(id, name);
            }
        };
        await dialog.ShowAsync();
    }

    private static ListViewItem Row(string title, string subtitle, object tag)
    {
        var grid = new Grid { ColumnSpacing = 12 };
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        var text = new StackPanel { Spacing = 2 };
        text.Children.Add(new TextBlock { Text = title });
        if (subtitle.Length > 0)
        {
            text.Children.Add(new TextBlock { Text = subtitle, Style = (Style)Application.Current.Resources["Caption"] });
        }
        grid.Children.Add(text);
        var add = new FontIcon { Glyph = "", FontSize = 14, VerticalAlignment = VerticalAlignment.Center };
        Grid.SetColumn(add, 1);
        grid.Children.Add(add);
        return new ListViewItem { Content = grid, Tag = tag };
    }
}
