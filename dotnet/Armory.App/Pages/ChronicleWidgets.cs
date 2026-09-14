using System.Globalization;
using Armory.Client.Shell;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using Windows.UI;

namespace Armory.App.Pages;

/// <summary>
/// The drawn things the chronicle, run and character pages share: prose in a
/// serif, figures in a monospace, the momentum strip, the ledger bar and the
/// weekday bars. Kept beside the pages that use them rather than in
/// <see cref="Widgets"/>, which is every page's vocabulary.
/// </summary>
internal static class ChronicleWidgets
{
    private static readonly FontFamily Serif = new("Georgia, Cambria, Times New Roman");
    private static readonly FontFamily Mono = new("Cascadia Mono, Consolas");

    private static Style Style(string key) => (Style)Application.Current.Resources[key];

    private static Brush Brush(string key) => (Brush)Application.Current.Resources[key];

    /// <summary>Narrative, in a serif. Paragraphs separated by a blank line are set with a paragraph gap rather than an empty line of prose.</summary>
    public static StackPanel Prose(string body, bool italic = false)
    {
        var stack = new StackPanel { Spacing = 10 };
        foreach (var paragraph in body.Replace("\r\n", "\n", StringComparison.Ordinal).Split("\n\n", StringSplitOptions.RemoveEmptyEntries))
        {
            stack.Children.Add(new TextBlock
            {
                Text = paragraph.Trim(),
                TextWrapping = TextWrapping.Wrap,
                FontFamily = Serif,
                FontSize = 15,
                LineHeight = 24,
                FontStyle = italic ? Windows.UI.Text.FontStyle.Italic : Windows.UI.Text.FontStyle.Normal,
            });
        }
        return stack;
    }

    /// <summary>A serif title, the size a diary page heads itself in.</summary>
    public static TextBlock SerifTitle(string text, double size = 20) => new()
    {
        Text = text,
        TextWrapping = TextWrapping.Wrap,
        FontFamily = Serif,
        FontSize = size,
        LineHeight = size * 1.4,
        FontWeight = Microsoft.UI.Text.FontWeights.SemiBold,
    };

    /// <summary>A figure in the monospace, so a column of them lines up.</summary>
    public static TextBlock Figure(string text, bool accent = false, double size = 14)
    {
        var block = new TextBlock { Text = text, FontFamily = Mono, FontSize = size, VerticalAlignment = VerticalAlignment.Center };
        if (accent)
        {
            block.Foreground = Widgets.AccentBrush;
        }
        return block;
    }

    /// <summary>One labelled fact: a 108px tertiary label beside the value, the way the handoff's "What happened" list is set.</summary>
    public static Grid Fact(string label, string value)
    {
        var row = new Grid { ColumnSpacing = 12, Margin = new Thickness(0, 4, 0, 4) };
        row.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(108) });
        row.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        row.Children.Add(new TextBlock { Text = label, Style = Style("Caption"), VerticalAlignment = VerticalAlignment.Top, TextWrapping = TextWrapping.Wrap });
        var text = new TextBlock { Text = value, TextWrapping = TextWrapping.Wrap };
        Grid.SetColumn(text, 1);
        row.Children.Add(text);
        return row;
    }

    /// <summary>A small labelled pill. The accent one is the stop the evening was spent at.</summary>
    public static Border Chip(string text, bool accent = false)
    {
        var chip = new Border
        {
            CornerRadius = new CornerRadius(4),
            Padding = new Thickness(8, 3, 8, 3),
            Background = Brush(accent ? "AccentFillColorDefaultBrush" : "ControlFillColorDefaultBrush"),
            BorderBrush = Brush("ControlStrokeColorDefaultBrush"),
            BorderThickness = new Thickness(1),
            Child = new TextBlock { Text = text, FontSize = 12 },
        };
        if (accent)
        {
            ((TextBlock)chip.Child).Foreground = Brush("TextOnAccentFillColorPrimaryBrush");
        }
        return chip;
    }

    /// <summary>
    /// The last fourteen days, one bar each. A day nobody played is drawn as
    /// a dot on the baseline, not a zero-height bar: somebody who did not
    /// play on Tuesday did not play a very little on Tuesday. The last day is
    /// the accent, the rest are quiet.
    /// </summary>
    public static Grid Momentum(IReadOnlyList<double?> days, double height = 36)
    {
        var strip = new Grid { ColumnSpacing = 3, Height = height, VerticalAlignment = VerticalAlignment.Bottom };
        for (var index = 0; index < days.Count; index++)
        {
            strip.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
            var last = index == days.Count - 1;
            Border bar = days[index] is { } share
                ? new Border
                {
                    Height = Math.Max(3, share * height),
                    VerticalAlignment = VerticalAlignment.Bottom,
                    CornerRadius = new CornerRadius(2),
                    Background = last ? Widgets.AccentBrush : Brush("ControlStrongFillColorDefaultBrush"),
                    Opacity = last ? 1 : 0.7,
                }
                : new Border
                {
                    Height = 2,
                    VerticalAlignment = VerticalAlignment.Bottom,
                    Background = Brush("ControlStrokeColorDefaultBrush"),
                };
            Grid.SetColumn(bar, index);
            strip.Children.Add(bar);
        }
        return strip;
    }

    /// <summary>Seven bars, one a weekday, tallest at the commonest. The commonest is the accent.</summary>
    public static Grid Weekdays(int[] counts, int modal, double height = 44)
    {
        var strip = new Grid { ColumnSpacing = 6 };
        var most = Math.Max(counts.Max(), 1);
        var letters = new[] { "M", "T", "W", "T", "F", "S", "S" };
        for (var index = 0; index < 7; index++)
        {
            strip.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
            var day = new StackPanel { Spacing = 5 };
            var well = new Grid { Height = height };
            well.Children.Add(new Border
            {
                Height = Math.Max(2, counts[index] / (double)most * height),
                VerticalAlignment = VerticalAlignment.Bottom,
                CornerRadius = new CornerRadius(2),
                Background = index == modal ? Widgets.AccentBrush : Brush("ControlStrongFillColorDefaultBrush"),
                Opacity = index == modal || counts[index] == 0 ? 1 : 0.7,
            });
            day.Children.Add(well);
            var letter = Figure(letters[index], accent: index == modal, size: 12);
            letter.HorizontalAlignment = HorizontalAlignment.Center;
            day.Children.Add(letter);
            Grid.SetColumn(day, index);
            strip.Children.Add(day);
        }
        return strip;
    }

    /// <summary>
    /// Where the gold went, as one bar: the income's segments then the
    /// spending's, each book's first segment at full strength. The shares
    /// are of everything that moved, so the bar is the shape of the
    /// evening's money rather than of either half.
    /// </summary>
    public static Grid Ledger(IReadOnlyList<Segment> income, IReadOnlyList<Segment> spending)
    {
        var bar = new Grid { Height = 8, ColumnSpacing = 1 };
        var column = 0;
        foreach (var (book, brush) in new[] { (income, "SystemFillColorSuccessBrush"), (spending, "SystemFillColorCriticalBrush") })
        {
            foreach (var segment in book)
            {
                bar.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(Math.Max(segment.Share, 0.001), GridUnitType.Star) });
                var piece = new Border { Background = Brush(brush), Opacity = segment.First ? 1 : 0.5, CornerRadius = new CornerRadius(2) };
                Grid.SetColumn(piece, column++);
                bar.Children.Add(piece);
            }
        }
        return bar;
    }

    /// <summary>A number that is up or down, coloured the way a ledger colours it.</summary>
    public static TextBlock Net(long copper)
    {
        var word = copper < 0 ? "down" : "up";
        var block = Figure($"{word} {Cards.Gold(Math.Abs(copper))}", size: 12);
        block.Foreground = Brush(copper < 0 ? "SystemFillColorCriticalBrush" : "SystemFillColorSuccessBrush");
        return block;
    }

    /// <summary>A line of small print in the monospace, uppercased, the way the almanac sets a meta line.</summary>
    public static TextBlock Meta(string text)
    {
        var block = Figure(text.ToUpperInvariant(), size: 11);
        block.Foreground = Widgets.Tertiary;
        block.TextWrapping = TextWrapping.Wrap;
        return block;
    }

    /// <summary>A day over its month, for the list's gutter.</summary>
    public static StackPanel DateMarker(DateTimeOffset at, bool newest)
    {
        var block = new StackPanel { Width = 44, HorizontalAlignment = HorizontalAlignment.Right };
        var local = at.ToLocalTime();
        var day = Figure(local.ToString("dd", CultureInfo.InvariantCulture), accent: newest, size: 18);
        day.HorizontalAlignment = HorizontalAlignment.Right;
        block.Children.Add(day);
        var month = Meta(local.ToString("MMM", CultureInfo.InvariantCulture));
        month.HorizontalAlignment = HorizontalAlignment.Right;
        block.Children.Add(month);
        return block;
    }

    /// <summary>A quiet hairline between blocks.</summary>
    public static Border Hairline() => new() { Height = 1, Background = Brush("DividerStrokeColorDefaultBrush"), Margin = new Thickness(0, 4, 0, 4) };

    /// <summary>A colour for a class, or nothing for "Everyone".</summary>
    public static Color? ClassColour(string className, bool light) =>
        className.Length == 0 ? null : ((SolidColorBrush)Widgets.ClassBrush(className, light)).Color;
}
