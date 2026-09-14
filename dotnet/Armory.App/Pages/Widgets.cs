using Armory.Client.Roster;
using Armory.Roster;
using Microsoft.UI;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using Microsoft.UI.Xaml.Shapes;
using Windows.UI;

namespace Armory.App.Pages;

/// <summary>The handful of drawn things every page shares, built in code so they cannot drift.</summary>
public static class Widgets
{
    private static Style Style(string key) => (Style)Application.Current.Resources[key];

    private static Brush Brush(string key) => (Brush)Application.Current.Resources[key];

    /// <summary>
    /// Blizzard's class colours, read off <see cref="Classes"/> so the table
    /// is testable. Fixed game data, not theme values, and never swapped for
    /// the accent. Priest and Rogue fail contrast on a light card and are
    /// darkened there rather than reassigned.
    /// </summary>
    public static Brush ClassBrush(string className, bool light)
    {
        var ring = Classes.Ring(className);
        var colour = light ? ring.Light : ring.Dark;
        return new SolidColorBrush(Color.FromArgb(255, colour.R, colour.G, colour.B));
    }

    public static bool IsLight(FrameworkElement element) => element.ActualTheme == ElementTheme.Light;

    /// <summary>A page header: title over subtitle, commands right-aligned on the same baseline row.</summary>
    public static Grid Header(string title, string subtitle, params UIElement[] commands)
    {
        var grid = new Grid { ColumnSpacing = 16, Margin = new Thickness(0, 0, 0, 16) };
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        var text = new StackPanel { Spacing = 2 };
        text.Children.Add(new TextBlock { Text = title, Style = Style("PageTitle") });
        text.Children.Add(new TextBlock { Text = subtitle, Style = Style("PageSubtitle") });
        grid.Children.Add(text);
        var row = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 8, VerticalAlignment = VerticalAlignment.Top };
        foreach (var command in commands)
        {
            row.Children.Add(command);
        }
        Grid.SetColumn(row, 1);
        grid.Children.Add(row);
        return grid;
    }

    public static Button Accent(string text, string? glyph = null)
    {
        var button = new Button { Style = (Style)Application.Current.Resources["AccentButtonStyle"], MinHeight = 34 };
        if (glyph is null)
        {
            button.Content = text;
        }
        else
        {
            var content = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 8 };
            content.Children.Add(new FontIcon { Glyph = glyph, FontSize = 16 });
            content.Children.Add(new TextBlock { Text = text });
            button.Content = content;
        }
        return button;
    }

    public static Button Standard(string text) => new() { Content = text, MinHeight = 34 };

    public static Border Card(UIElement content, bool dense = false)
    {
        var card = new Border { Style = Style("Card"), Child = content };
        if (dense)
        {
            card.Padding = (Thickness)Application.Current.Resources["DenseCardPadding"];
        }
        return card;
    }

    public static TextBlock Text(string text, string style = "Body")
    {
        var block = new TextBlock { Text = text, TextWrapping = TextWrapping.Wrap };
        if (style != "Body")
        {
            block.Style = Style(style);
        }
        return block;
    }

    /// <summary>A card heading with an optional caption on the right, the way every card in the handoff opens.</summary>
    public static Grid CardHead(string heading, string? aside = null)
    {
        var grid = new Grid { Margin = new Thickness(0, 0, 0, 10) };
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        grid.Children.Add(new TextBlock { Text = heading, Style = Style("CardHeading") });
        if (aside is not null)
        {
            var caption = new TextBlock { Text = aside, Style = Style("Caption"), VerticalAlignment = VerticalAlignment.Bottom };
            Grid.SetColumn(caption, 1);
            grid.Children.Add(caption);
        }
        return grid;
    }

    /// <summary>A stat card: caption over figure over sub-line.</summary>
    public static Border Stat(string caption, string figure, string? note = null)
    {
        var stack = new StackPanel { Spacing = 2 };
        stack.Children.Add(new TextBlock { Text = caption, Style = Style("Caption") });
        stack.Children.Add(new TextBlock { Text = figure, Style = Style("Figure") });
        if (note is not null)
        {
            stack.Children.Add(new TextBlock { Text = note, Style = Style("Caption") });
        }
        return Card(stack, dense: true);
    }

    /// <summary>An InfoBar for a standing condition, in the content flow.</summary>
    public static InfoBar Standing(string title, string message, InfoBarSeverity severity = InfoBarSeverity.Informational) => new()
    {
        Title = title,
        Message = message,
        Severity = severity,
        IsOpen = true,
        // A standing condition is not dismissed; it goes away when it stops being true.
        IsClosable = false,
        Margin = new Thickness(0, 0, 0, 16),
    };

    /// <summary>The grey box with a star, which is the intended pre-load state of every piece of art.</summary>
    public static Border ArtPlaceholder(bool small = false, bool dimmed = false)
    {
        var box = new Border
        {
            Style = Style(small ? "SmallArtBox" : "ArtBox"),
            Child = new FontIcon { Glyph = "", FontSize = small ? 14 : 28, Opacity = 0.22, HorizontalAlignment = HorizontalAlignment.Center, VerticalAlignment = VerticalAlignment.Center },
        };
        if (dimmed)
        {
            // The art box only, never the tile: dimming the whole tile
            // multiplies into the caption's alpha and fails contrast.
            box.Opacity = 0.45;
        }
        return box;
    }

    /// <summary>A thin accent progress bar.</summary>
    public static ProgressBar Bar(double fraction, bool inherited = false)
    {
        var bar = new ProgressBar { Style = Style("ThinBar"), Minimum = 0, Maximum = 1, Value = fraction };
        if (inherited)
        {
            // Never the accent for a standing the run did not earn.
            bar.Foreground = new SolidColorBrush(Color.FromArgb(89, 255, 255, 255));
        }
        return bar;
    }

    /// <summary>
    /// A character's own render if there is one, and their class crest if
    /// not, inside a ring in the class colour. The crest is a fallback rather
    /// than a placeholder: a smaller picture of a true thing. The picture is
    /// painted as an ellipse's fill so the edge is a circle's whatever the
    /// picture's own shape, and the placeholder is the grey box every other
    /// piece of art starts as, rounded to fit the ring.
    /// </summary>
    public static Grid Portrait(Armory.Client.Images.Images art, string url, string className, bool light, int size, string? tooltip = null)
    {
        var holder = new Grid { Width = size, Height = size, VerticalAlignment = VerticalAlignment.Center };
        var placeholder = ArtPlaceholder(small: true);
        placeholder.Width = size;
        placeholder.Height = size;
        placeholder.CornerRadius = new CornerRadius(size / 2.0);
        holder.Children.Add(placeholder);
        var picture = new Ellipse { Width = size, Height = size };
        holder.Children.Add(picture);
        holder.Children.Add(new Ellipse { Width = size, Height = size, Stroke = ClassBrush(className, light), StrokeThickness = 2 });
        if (tooltip is not null)
        {
            ToolTipService.SetToolTip(holder, tooltip);
        }
        _ = ShowPortrait(art, placeholder, picture, url, size);
        return holder;
    }

    /// <summary>Fetch the picture once the holder is built. A portrait that cannot be had keeps its placeholder, which already reads correctly.</summary>
    private static async Task ShowPortrait(Armory.Client.Images.Images art, Border placeholder, Ellipse picture, string url, int size)
    {
        // Decoded at twice the drawn size, so a scaled display is not handed a soft picture.
        var decoded = await ArtLoader.Decode(art, url, size * 2);
        if (decoded is null)
        {
            return;
        }
        picture.Fill = new ImageBrush { ImageSource = decoded, Stretch = Stretch.UniformToFill };
        placeholder.Visibility = Visibility.Collapsed;
    }

    public static Brush Tertiary => Brush("TextFillColorTertiaryBrush");

    public static Brush AccentBrush => Brush("AccentFillColorDefaultBrush");

    /// <summary>A dot in a character's class colour before their name.</summary>
    public static StackPanel Named(Character? character, string fallback, bool light)
    {
        var row = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 6 };
        if (character is not null)
        {
            row.Children.Add(new Ellipse { Width = 6, Height = 6, Fill = ClassBrush(character.Class, light), VerticalAlignment = VerticalAlignment.Center });
        }
        row.Children.Add(new TextBlock { Text = character?.DisplayName ?? fallback, Style = Style("Caption") });
        return row;
    }
}
