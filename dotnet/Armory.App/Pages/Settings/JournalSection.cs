using Armory.Chronicle;
using Armory.Client.Shell;
using Armory.Settings;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;

namespace Armory.App.Pages.Settings;

/// <summary>The Journal card on Settings: what writes the entries, whether it answers, and whether entries are written unasked. Saves itself, so the page's own Save never has to know about it.</summary>
public static class JournalSection
{
    public static Border Build(Account account, MainWindow window)
    {
        var journal = new StackPanel { Spacing = 10 };
        journal.Children.Add(Widgets.CardHead("Journal", account.JournalModel is { } named ? $"answering: {named}" : Describe(account.Settings.JournalBackend)));

        var backend = new RadioButtons { Header = "Written by" };
        backend.Items.Add("A llama-server on this machine");
        backend.Items.Add("Claude Code, signed in on this machine");
        backend.SelectedIndex = account.Settings.JournalBackend == JournalBackend.ClaudeCode ? 1 : 0;
        journal.Children.Add(backend);

        var server = new TextBox { Header = "Server", Text = account.Settings.JournalServer, PlaceholderText = Journal.DefaultServer };
        var model = new TextBox { Header = "Model", Text = account.Settings.JournalModel, PlaceholderText = Armory.Settings.Settings.DefaultJournalModel };
        var automatic = new ToggleSwitch { Header = "Write every new evening without being asked", IsOn = account.Settings.JournalAutomatic, OnContent = "On", OffContent = "Off" };
        journal.Children.Add(server);
        journal.Children.Add(model);
        journal.Children.Add(automatic);
        var status = new TextBlock { Style = (Style)Application.Current.Resources["Caption"], TextWrapping = TextWrapping.Wrap };
        status.Text = account.JournalReady
            ? $"Answering: {account.JournalModel}."
            : NothingYet(account.Settings.JournalBackend);
        journal.Children.Add(status);

        JournalBackend Chosen() => backend.SelectedIndex == 1 ? JournalBackend.ClaudeCode : JournalBackend.LlamaServer;
        void Show()
        {
            server.IsEnabled = Chosen() == JournalBackend.LlamaServer;
            model.IsEnabled = Chosen() == JournalBackend.ClaudeCode;
        }
        Show();
        backend.SelectionChanged += (_, _) => Show();

        var buttons = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 8 };
        var test = Widgets.Standard("Test");
        test.Click += async (_, _) =>
        {
            status.Text = "Checking…";
            var answered = await account.IdentifyJournal(Address(server), Chosen());
            status.Text = answered is null ? NothingYet(Chosen()) : $"Answering: {answered}.";
        };
        var save = Widgets.Standard("Save journal settings");
        save.Click += async (_, _) =>
        {
            await account.SaveJournalSettings(Address(server), automatic.IsOn, Chosen(), model.Text);
            window.Say("Journal settings saved.");
        };
        buttons.Children.Add(test);
        buttons.Children.Add(save);
        journal.Children.Add(buttons);
        return Widgets.Card(journal);
    }

    private static string Describe(JournalBackend backend) =>
        backend == JournalBackend.ClaudeCode ? "Claude Code on this machine" : "a local llama-server";

    private static string NothingYet(JournalBackend backend) =>
        backend == JournalBackend.ClaudeCode
            ? "Nobody has answered yet. Run claude in a terminal and sign in with your subscription, then test."
            : "Nothing has answered yet. Test the address, and check that llama-server is running.";

    private static string Address(TextBox server)
    {
        var text = server.Text.Trim();
        return text.Length == 0 ? Journal.DefaultServer : text;
    }
}
