using System.Reflection;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media.Imaging;

namespace Armory.App.Dialogs;

/// <summary>
/// About Armory: the port of the GTK <c>AdwAboutDialog</c>. The mark, the
/// name, the version, the developer, the two sentences of comment and the
/// licence line, in that order, because that is the order the GTK one reads
/// in. No website and no issue tracker, because the GTK one names neither.
/// </summary>
public sealed partial class AboutDialog : ContentDialog
{
    public const string Comments =
        "A World of Warcraft companion, built around replaying content an account already remembers.\n\n" +
        "Profile data comes from Blizzard and is a snapshot taken when a character logs out, never a live view.";

    public const string Developer = "Matthew Hagrelius";

    public static readonly Uri LicenceUrl = new("https://www.gnu.org/licenses/gpl-3.0.html");

    public AboutDialog()
    {
        Title = "About Armory";
        CloseButtonText = "Close";
        DefaultButton = ContentDialogButton.Close;

        var column = new StackPanel { Spacing = 12, MinWidth = 400, MaxWidth = 460 };

        var mark = new Image { Width = 64, Height = 64, HorizontalAlignment = HorizontalAlignment.Center };
        var icon = Path.Combine(AppContext.BaseDirectory, "Assets", "Armory.ico");
        if (File.Exists(icon))
        {
            mark.Source = new BitmapImage(new Uri(icon));
        }
        column.Children.Add(mark);

        column.Children.Add(new TextBlock
        {
            Text = "Armory",
            Style = (Style)Application.Current.Resources["Subtitle"],
            HorizontalAlignment = HorizontalAlignment.Center,
        });
        column.Children.Add(new TextBlock
        {
            Text = Developer,
            Style = (Style)Application.Current.Resources["Secondary"],
            HorizontalAlignment = HorizontalAlignment.Center,
        });
        column.Children.Add(new TextBlock
        {
            Text = Version(),
            Style = (Style)Application.Current.Resources["Caption"],
            HorizontalAlignment = HorizontalAlignment.Center,
        });

        column.Children.Add(new TextBlock
        {
            Text = Comments,
            TextWrapping = TextWrapping.Wrap,
            Margin = new Thickness(0, 8, 0, 0),
        });

        var licence = new TextBlock
        {
            TextWrapping = TextWrapping.Wrap,
            Style = (Style)Application.Current.Resources["Caption"],
            Margin = new Thickness(0, 4, 0, 0),
        };
        licence.Inlines.Add(new Microsoft.UI.Xaml.Documents.Run { Text = "This application comes with absolutely no warranty. See the " });
        var link = new Microsoft.UI.Xaml.Documents.Hyperlink { NavigateUri = LicenceUrl };
        link.Inlines.Add(new Microsoft.UI.Xaml.Documents.Run { Text = "GNU General Public License, version 3 or later" });
        licence.Inlines.Add(link);
        licence.Inlines.Add(new Microsoft.UI.Xaml.Documents.Run { Text = " for details." });
        column.Children.Add(licence);

        Content = column;
    }

    /// <summary>
    /// The assembly's version, as the GTK reads <c>CARGO_PKG_VERSION</c>. The
    /// informational version is what a <c>&lt;Version&gt;</c> in the project
    /// sets; the build hash it may carry after a plus is not a version
    /// anybody is asked for.
    /// </summary>
    public static string Version()
    {
        var assembly = typeof(AboutDialog).Assembly;
        var informational = assembly.GetCustomAttribute<AssemblyInformationalVersionAttribute>()?.InformationalVersion;
        if (!string.IsNullOrWhiteSpace(informational))
        {
            var plus = informational.IndexOf('+', StringComparison.Ordinal);
            return plus < 0 ? informational : informational[..plus];
        }
        return assembly.GetName().Version?.ToString(3) ?? "unknown";
    }
}
