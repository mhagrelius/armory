using Armory.Chronicle;
using Armory.Client.Shell;
using Armory.Settings;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;

namespace Armory.App.Dialogs;

/// <summary>
/// Journal setup: what writes the entries, whether it answers, and whether
/// entries are written without being asked. The Chronicle records every
/// session on its own and needs nothing set up to do it; this is only for
/// turning one into prose.
/// </summary>
public sealed partial class JournalDialog : ContentDialog
{
    private readonly Account account;
    private readonly RadioButtons backend;
    private readonly TextBox address;
    private readonly TextBox model;
    private readonly ToggleSwitch automatic;
    private readonly TextBlock status;
    private readonly TextBlock statusDetail;
    private readonly TextBlock explanation;

    public JournalDialog(Account account)
    {
        this.account = account;
        Title = "Journal";
        PrimaryButtonText = "Save";
        CloseButtonText = "Cancel";
        DefaultButton = ContentDialogButton.Primary;

        var column = new StackPanel { Spacing = 14, MinWidth = 420 };
        column.Children.Add(new TextBlock
        {
            Text = "The Chronicle records every session on its own and needs nothing set up to do it. This is only for turning one into prose.",
            TextWrapping = TextWrapping.Wrap,
            Style = (Style)Application.Current.Resources["Secondary"],
        });

        backend = new RadioButtons { Header = "Written by" };
        backend.Items.Add("A llama-server on this machine");
        backend.Items.Add("Claude Code, signed in on this machine");
        backend.SelectedIndex = account.Settings.JournalBackend == JournalBackend.ClaudeCode ? 1 : 0;
        column.Children.Add(backend);

        explanation = new TextBlock { TextWrapping = TextWrapping.Wrap, Style = (Style)Application.Current.Resources["Caption"] };
        column.Children.Add(explanation);

        address = new TextBox { Header = "Server — llama.cpp's OpenAI-compatible endpoint", Text = account.Settings.JournalServer, PlaceholderText = Journal.DefaultServer };
        column.Children.Add(address);

        model = new TextBox { Header = "Model — an alias like sonnet or opus, or a full model id", Text = account.Settings.JournalModel, PlaceholderText = Armory.Settings.Settings.DefaultJournalModel };
        column.Children.Add(model);

        var check = new Grid { ColumnSpacing = 12 };
        check.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        check.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        var lines = new StackPanel { Spacing = 2, VerticalAlignment = VerticalAlignment.Center };
        status = new TextBlock { Style = (Style)Application.Current.Resources["BodyStrongTextBlockStyle"] };
        statusDetail = new TextBlock { Style = (Style)Application.Current.Resources["Caption"], TextWrapping = TextWrapping.Wrap };
        lines.Children.Add(status);
        lines.Children.Add(statusDetail);
        check.Children.Add(lines);
        var test = new Button { Content = "Test", VerticalAlignment = VerticalAlignment.Center };
        test.Click += async (_, _) =>
        {
            // Tested against what is in the dialog rather than what is
            // saved, so somebody can check a choice before committing to it.
            SetStatus("Checking…", Backend() == JournalBackend.ClaudeCode ? "Asking claude who is signed in." : Address());
            var named = await account.IdentifyJournal(Address(), Backend());
            if (named is not null)
            {
                SetStatus("Answering", named);
            }
            else if (Backend() == JournalBackend.ClaudeCode)
            {
                SetStatus("Nobody signed in", "Open a terminal, run claude, and sign in with your subscription. Then test again.");
            }
            else
            {
                SetStatus("Nothing answered", "Check the address, and that llama-server is running.");
            }
        };
        Grid.SetColumn(test, 1);
        check.Children.Add(test);
        column.Children.Add(check);

        automatic = new ToggleSwitch { Header = "Write entries automatically", IsOn = account.Settings.JournalAutomatic, OnContent = "On", OffContent = "Off" };
        column.Children.Add(automatic);
        column.Children.Add(new TextBlock
        {
            Text = "Write up each new evening as soon as Armory reads it. On by default — a journal you have to remember to write is one that does not get written.",
            TextWrapping = TextWrapping.Wrap,
            Style = (Style)Application.Current.Resources["Caption"],
        });
        Content = new ScrollViewer { Content = column };

        if (account.JournalModel is { } named)
        {
            SetStatus("Answering", named);
        }
        else
        {
            SetStatus("Not checked yet", "Test to see what answers.");
        }
        ShowChoice();
        backend.SelectionChanged += (_, _) => ShowChoice();

        PrimaryButtonClick += async (_, args) =>
        {
            var deferral = args.GetDeferral();
            // The choice may have changed, so what is answering may have
            // too; saving re-identifies.
            await account.SaveJournalSettings(Address(), automatic.IsOn, Backend(), model.Text);
            deferral.Complete();
        };
    }

    private JournalBackend Backend() => backend.SelectedIndex == 1 ? JournalBackend.ClaudeCode : JournalBackend.LlamaServer;

    private string Address()
    {
        var text = address.Text.Trim();
        return text.Length == 0 ? Journal.DefaultServer : text;
    }

    /// <summary>The field the chosen backend reads, and what it costs. The other field stays, disabled, so switching back loses nothing.</summary>
    private void ShowChoice()
    {
        var claude = Backend() == JournalBackend.ClaudeCode;
        address.IsEnabled = !claude;
        model.IsEnabled = claude;
        explanation.Text = claude
            ? "Uses the claude command-line and whatever it is signed in to — your own subscription, on this machine. The brief for each evening is sent to Anthropic to be written; nothing else is, and Armory never sees the login."
            : "llama.cpp's server on this machine. Nothing is sent anywhere and nothing is billed.";
    }

    private void SetStatus(string title, string detail)
    {
        status.Text = title;
        statusDetail.Text = detail;
    }
}
