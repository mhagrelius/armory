using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Windows.Foundation;

namespace Armory.App.Pages;

/// <summary>
/// A row of children that wraps, because WinUI ships no wrapping panel and a
/// row of factions or bosses that runs off the side of the pane is a row
/// nobody can read.
/// </summary>
public sealed partial class Wrap : Panel
{
    public double Spacing { get; set; } = 6;

    protected override Size MeasureOverride(Size availableSize)
    {
        var width = double.IsInfinity(availableSize.Width) ? double.MaxValue : availableSize.Width;
        double x = 0, y = 0, row = 0, widest = 0;
        foreach (var child in Children)
        {
            child.Measure(new Size(width, availableSize.Height));
            var size = child.DesiredSize;
            if (x > 0 && x + size.Width > width)
            {
                y += row + Spacing;
                x = 0;
                row = 0;
            }
            x += size.Width + Spacing;
            row = Math.Max(row, size.Height);
            widest = Math.Max(widest, x - Spacing);
        }
        return new Size(double.IsInfinity(availableSize.Width) ? widest : Math.Min(widest, width), y + row);
    }

    protected override Size ArrangeOverride(Size finalSize)
    {
        double x = 0, y = 0, row = 0;
        foreach (var child in Children)
        {
            var size = child.DesiredSize;
            if (x > 0 && x + size.Width > finalSize.Width)
            {
                y += row + Spacing;
                x = 0;
                row = 0;
            }
            child.Arrange(new Rect(x, y, size.Width, size.Height));
            x += size.Width + Spacing;
            row = Math.Max(row, size.Height);
        }
        return finalSize;
    }
}
