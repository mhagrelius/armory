using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;

namespace Armory.App.Pages;

/// <summary>A place whose page is not built yet. Says so rather than showing nothing.</summary>
public sealed partial class ComingPage : Page
{
    public ComingPage(string title)
    {
        Content = new StackPanel
        {
            Padding = (Thickness)Application.Current.Resources["PagePadding"],
            Spacing = 8,
            Children =
            {
                new TextBlock { Text = title, Style = (Style)Application.Current.Resources["PageTitle"] },
                new TextBlock { Text = "Nothing here yet — this page arrives with its slice of the port.", Style = (Style)Application.Current.Resources["PageSubtitle"] },
            },
        };
    }
}
