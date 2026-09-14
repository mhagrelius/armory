using System.Globalization;
using Armory.App.Pages;
using Armory.Client.Shell;
using Armory.Collections;
using Armory.Roster;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;

namespace Armory.App.Dialogs;

/// <summary>
/// One mount, pet, toy or piece of decor: the render, where it comes from
/// in the journal's own words, what it says about itself, somewhere to read
/// more, and the numbers. The port of <c>ui/collectible_dialog.rs</c>;
/// undesigned in the handoff, so it is a plain Fluent content dialog.
/// </summary>
public static class CollectibleDialog
{
    /// <summary>Bigger than the grid's thumbnail and fetched separately: the same URL at two sizes is two textures.</summary>
    private const int Art = 220;

    public static async Task Show(XamlRoot root, Account account, Collectible entry, bool owned)
    {
        var column = new StackPanel { Spacing = 18 };
        column.Children.Add(Heading(account, entry, owned));
        column.Children.Add(Provenance(entry));
        if (entry.Flavour is { Length: > 0 } flavour)
        {
            var group = Group("Description");
            group.Children.Add(new TextBlock { Text = flavour, TextWrapping = TextWrapping.Wrap, FontStyle = Windows.UI.Text.FontStyle.Italic });
            column.Children.Add(group);
        }
        column.Children.Add(Links(entry));
        column.Children.Add(Identifiers(entry));

        var dialog = new ContentDialog
        {
            Title = entry.Name.Length > 0 ? entry.Name : string.Create(CultureInfo.InvariantCulture, $"{entry.Kind.Singular()} {entry.Id}"),
            Content = new ScrollViewer { Content = column, MaxHeight = 640 },
            CloseButtonText = "Close",
            DefaultButton = ContentDialogButton.Close,
            XamlRoot = root,
        };
        await dialog.ShowAsync();
    }

    /// <summary>The render, the name, and whether it is already had.</summary>
    private static StackPanel Heading(Account account, Collectible entry, bool owned)
    {
        var stack = new StackPanel { Spacing = 6, HorizontalAlignment = HorizontalAlignment.Center };
        var box = new Border
        {
            Width = Art,
            Height = Art,
            CornerRadius = new CornerRadius(8),
            Background = (Brush)Application.Current.Resources["ControlFillColorDefaultBrush"],
            Child = new FontIcon { Glyph = "", FontSize = 48, Opacity = 0.22, HorizontalAlignment = HorizontalAlignment.Center, VerticalAlignment = VerticalAlignment.Center },
        };
        stack.Children.Add(box);
        if (account.ArtUrl(entry) is { } url)
        {
            _ = Paint(box, account, url);
        }

        var status = owned
            ? "Collected"
            : entry.Source == Source.Promotion ? "Not collected — and no longer obtainable" : "Not collected";
        stack.Children.Add(Widgets.Text(status, "Secondary"));
        // A faction lock is the difference between "you are missing this" and
        // "this was never yours to have", and only the game says so.
        if (entry.Faction is { } faction)
        {
            stack.Children.Add(Widgets.Text($"{faction.Label()} only", "Caption"));
        }
        return stack;
    }

    private static async Task Paint(Border box, Account account, string url)
    {
        if (await ArtLoader.Decode(account.Art, url, Art) is { } picture)
        {
            box.Child = null;
            box.Background = new ImageBrush { ImageSource = picture, Stretch = Stretch.UniformToFill };
        }
    }

    /// <summary>Where it comes from, as the game's own journal words it: several labelled lines, each its own row.</summary>
    private static StackPanel Provenance(Collectible entry)
    {
        var group = Group("Where it comes from");
        var text = entry.Description is { Length: > 0 } description ? description : null;
        if (text is null)
        {
            group.Children.Add(Row("Not recorded", entry.Kind switch
            {
                // Worth saying which, because "unknown" for a toy is a gap in
                // the game's data rather than in ours.
                Kind.Toy => "The toy box does not record where a toy came from. The Wowhead link below does.",
                Kind.Decor => "Decor comes from quests, achievements, reputations, professions and boss drops, and the catalogue does not always say which.",
                _ => "The journal has no source text for this one. The Wowhead link below usually does.",
            }));
            return group;
        }
        foreach (var line in text.Split('\n').Where(line => line.Trim().Length > 0))
        {
            var colon = line.IndexOf(':', StringComparison.Ordinal);
            if (colon > 0 && line[(colon + 1)..].Trim().Length > 0)
            {
                group.Children.Add(Row(line[..colon].Trim(), line[(colon + 1)..].Trim()));
            }
            else
            {
                group.Children.Add(Row(line.Trim(), null));
            }
        }
        return group;
    }

    /// <summary>
    /// Somewhere to go for the pictures and the numbers. Wowhead is offered
    /// only where the id it would be looked up by is known: a toy or a piece
    /// of decor is addressed by the item it wraps, and sending somebody to a
    /// real, unrelated item is worse than sending them nowhere. Nothing is
    /// ever fetched from either.
    /// </summary>
    private static StackPanel Links(Collectible entry)
    {
        var group = Group("Read more", "Armory fetches nothing from these — Wowhead's terms forbid automated access — but they are where the model previews, drop rates and comments live.");
        if (entry.WowheadUrl() is { } wowhead)
        {
            group.Children.Add(Link("Wowhead", "Drop rates, comments, and a 3D preview", wowhead));
        }
        group.Children.Add(Link("Warcraft Wiki", "Community documentation and history", entry.WikiUrl()));
        return group;
    }

    /// <summary>The numbers, for anyone who wants to look something up themselves.</summary>
    private static StackPanel Identifiers(Collectible entry)
    {
        var group = Group("Identifiers", "The model id is what addresses the picture above on Blizzard's render service. The icon id addresses a file inside the game client's own archives, which has no URL at all.");
        group.Children.Add(Row(entry.Kind switch
        {
            Kind.Mount => "Mount ID",
            Kind.Pet => "Species ID",
            Kind.Toy => "Toy ID",
            _ => "Decor ID",
        }, entry.Id.ToString(CultureInfo.InvariantCulture)));
        group.Children.Add(Row(entry.Kind switch
        {
            Kind.Mount => "Spell ID",
            Kind.Pet => "Creature ID",
            _ => "Item ID",
        }, entry.LinkId.ToString(CultureInfo.InvariantCulture)));
        if (entry.Icon is { } icon)
        {
            group.Children.Add(Row("Icon file", icon.ToString(CultureInfo.InvariantCulture)));
        }
        if (entry.Display is { } display)
        {
            group.Children.Add(Row("Model", display.ToString(CultureInfo.InvariantCulture)));
        }
        return group;
    }

    private static StackPanel Group(string title, string? note = null)
    {
        var group = new StackPanel { Spacing = 6 };
        group.Children.Add(new TextBlock { Text = title, Style = (Style)Application.Current.Resources["CardHeading"] });
        if (note is not null)
        {
            group.Children.Add(Widgets.Text(note, "Caption"));
        }
        return group;
    }

    private static Border Row(string title, string? subtitle)
    {
        var stack = new StackPanel { Spacing = 2 };
        stack.Children.Add(new TextBlock { Text = title, TextWrapping = TextWrapping.Wrap });
        if (subtitle is not null)
        {
            stack.Children.Add(new TextBlock { Text = subtitle, Style = (Style)Application.Current.Resources["Caption"], TextWrapping = TextWrapping.Wrap, IsTextSelectionEnabled = true });
        }
        return new Border { Style = (Style)Application.Current.Resources["RowDivider"], Child = stack };
    }

    private static HyperlinkButton Link(string title, string subtitle, string url)
    {
        var stack = new StackPanel { Spacing = 2 };
        stack.Children.Add(new TextBlock { Text = title });
        stack.Children.Add(new TextBlock { Text = subtitle, Style = (Style)Application.Current.Resources["Caption"] });
        return new HyperlinkButton { Content = stack, NavigateUri = new Uri(url), Padding = new Thickness(0, 6, 0, 6), HorizontalAlignment = HorizontalAlignment.Stretch, HorizontalContentAlignment = HorizontalAlignment.Left };
    }
}
