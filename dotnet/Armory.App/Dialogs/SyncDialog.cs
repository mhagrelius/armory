using System.Globalization;
using Armory.App.Pages;
using Armory.Client.Sharing;
using Armory.Client.Shell;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;

namespace Armory.App.Dialogs;

/// <summary>
/// Account &amp; Sharing: one account, several machines. Which game folder
/// this machine reads and which account on the server that goes to, what is
/// queued to go up, what the last pass moved, and what the other end could
/// not read. Held open while a first sync runs past, so it redraws on every
/// pass.
/// </summary>
public sealed partial class SyncDialog : ContentDialog
{
    private readonly Account account;
    private readonly StackPanel body = new() { Spacing = 12, MinWidth = 560 };
    private readonly TextBox address;
    private readonly PasswordBox token;
    private readonly ComboBox serverAccount;
    private readonly ComboBox gameAccount;
    private readonly StackPanel waiting = new() { Spacing = 6 };
    private readonly TextBlock state = new() { Style = (Style)Application.Current.Resources["Caption"] };
    private readonly TextBlock lastPass = new() { Style = (Style)Application.Current.Resources["Caption"] };

    public SyncDialog(Account account)
    {
        this.account = account;
        Title = "Account & Sharing";
        PrimaryButtonText = "Sync now";
        SecondaryButtonText = "Send everything again";
        CloseButtonText = "Close";
        DefaultButton = ContentDialogButton.Close;

        body.Children.Add(Widgets.Text("One account, several machines", "Secondary"));

        var two = new Grid { ColumnSpacing = 12 };
        two.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        two.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        var machine = Widgets.Stat("This machine", Environment.MachineName, account.GameAccounts.Count > 1 ? "This install has been used by more than one game account" : "Game account: the only one this install has");
        var server = Widgets.Stat("Server", account.Settings.SyncUrl.Length == 0 ? "not set" : account.Settings.SyncUrl, "Plain HTTP over the tailnet");
        Grid.SetColumn(server, 1);
        two.Children.Add(machine);
        two.Children.Add(server);
        body.Children.Add(two);

        address = new TextBox { Header = "Address", Text = account.Settings.SyncUrl, PlaceholderText = "http://nas.example.ts.net:8084" };
        body.Children.Add(address);

        var fields = new Grid { ColumnSpacing = 12 };
        fields.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        fields.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        var tokenHeld = account.SecretHeld(SecretNames.SyncToken);
        token = new PasswordBox { Header = "Token", PlaceholderText = tokenHeld ? "One is held; type to replace it" : "The server's ARMORY_TOKEN" };
        serverAccount = new ComboBox { Header = "Account", IsEditable = true, Text = account.Settings.SyncAccount, PlaceholderText = "default", HorizontalAlignment = HorizontalAlignment.Stretch };
        Grid.SetColumn(serverAccount, 1);
        fields.Children.Add(token);
        fields.Children.Add(serverAccount);
        body.Children.Add(fields);
        body.Children.Add(Widgets.Text("Which one on the server this machine belongs to. The game account folder this machine reads is chosen below.", "Caption"));

        gameAccount = new ComboBox { Header = "Game account folder", HorizontalAlignment = HorizontalAlignment.Stretch };
        foreach (var name in account.GameAccounts)
        {
            gameAccount.Items.Add(name);
        }
        gameAccount.SelectedIndex = account.GameAccounts.IndexOf(account.Settings.WowAccount ?? "");
        // A chosen game account is a name for the server account too, but
        // only while that is still `default`: moving a machine that already
        // belongs to a named account is a different act.
        gameAccount.SelectionChanged += (_, _) =>
        {
            if (gameAccount.SelectedItem is not string folder)
            {
                return;
            }
            var offered = SharingWording.AccountFromFolder(folder);
            var showing = (serverAccount.Text ?? "").Trim();
            if (offered != SharingWording.DefaultAccount && (showing.Length == 0 || showing == SharingWording.DefaultAccount))
            {
                serverAccount.Text = offered;
            }
        };
        body.Children.Add(gameAccount);

        var queue = new StackPanel();
        queue.Children.Add(Widgets.CardHead("Waiting to go up", "what this machine has recorded and the server has not taken yet"));
        queue.Children.Add(waiting);
        queue.Children.Add(state);
        queue.Children.Add(lastPass);
        body.Children.Add(Widgets.Card(queue));

        var save = Widgets.Standard("Save");
        save.Click += async (_, _) =>
        {
            // The typed name would be refused by the server, which answers
            // 400 to a name it will not turn into a directory. Said here,
            // against the field, rather than as a failed pass an hour later.
            var typed = (serverAccount.Text ?? "").Trim();
            if (typed.Length > 0 && !SharingWording.ValidAccount(typed))
            {
                serverAccount.Header = "Account — letters, digits, dash, underscore and dot";
                serverAccount.Focus(FocusState.Programmatic);
                return;
            }
            serverAccount.Header = "Account";
            await account.SaveSyncTarget(new Chosen(
                address.Text,
                token.Password.Length == 0 ? null : token.Password,
                typed,
                gameAccount.SelectedItem as string));
            await Redraw();
        };
        // Deleting an account on the server sits beside Save rather than in the
        // button row, which has its three slots; it is the one destructive
        // thing here and it asks twice.
        var forget = Widgets.Standard("Delete on server…");
        forget.Click += async (_, _) => await ConfirmForget((serverAccount.Text ?? "").Trim() is { Length: > 0 } named ? named : SharingWording.DefaultAccount);
        var actions = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 8 };
        actions.Children.Add(save);
        actions.Children.Add(forget);
        body.Children.Add(actions);

        Content = new ScrollViewer { Content = body, MaxHeight = 640 };
        PrimaryButtonClick += async (_, args) =>
        {
            args.Cancel = true;
            await account.ShareNow();
            await Redraw();
        };
        SecondaryButtonClick += async (_, args) =>
        {
            args.Cancel = true;
            await ConfirmResend();
        };
        account.SyncStateChanged += () => _ = Redraw();
        _ = Redraw();
        _ = account.RefreshAccounts();
    }

    private async Task Redraw()
    {
        var now = await account.SyncStateNow();
        waiting.Children.Clear();
        if (now.Server.Length == 0)
        {
            state.Text = "Sharing is off. Set an address and a token above.";
        }
        else if (!now.TokenHeld)
        {
            state.Text = "Not sharing — the address is set but no token is held.";
        }
        else
        {
            state.Text = now.Passing ? "Running now…" : "State: sharing on";
        }

        if (now.Queued.Count == 0)
        {
            waiting.Children.Add(Widgets.Text(now.Server.Length == 0 ? "Not asked yet — run a pass and this fills in." : "Nothing waiting — everything this machine has recorded is on the server.", "Secondary"));
        }
        else
        {
            var grid = new Grid { ColumnSpacing = 24 };
            grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
            grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
            var left = new StackPanel();
            var right = new StackPanel();
            var index = 0;
            foreach (var (scope, count) in now.Queued)
            {
                var row = new Grid();
                row.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
                row.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
                row.Children.Add(new TextBlock { Text = SharingWording.Pretty(scope) });
                var figure = new TextBlock { Text = count.ToString("N0", CultureInfo.InvariantCulture), Style = (Style)Application.Current.Resources["BodyStrongTextBlockStyle"] };
                Grid.SetColumn(figure, 1);
                row.Children.Add(figure);
                (index++ % 2 == 0 ? left : right).Children.Add(row);
            }
            Grid.SetColumn(right, 1);
            grid.Children.Add(left);
            grid.Children.Add(right);
            waiting.Children.Add(grid);
            if (now.QueuedSince is { } since)
            {
                waiting.Children.Add(Widgets.Text($"Oldest waiting: {since}", "Caption"));
            }
        }

        // The sentence is the GTK dialog's, word for word; the row title is
        // this dialog's own.
        var status = new SharingStatus
        {
            Server = now.Server,
            Passing = now.Passing,
            Failures = now.Failures,
            Last = now.Last is { } pass
                ? new PassSummary(Account.When(pass.At, DateTimeOffset.UtcNow), pass.Sent, pass.Landed, pass.Removed, pass.Unreadable, pass.Failed)
                : null,
        };
        lastPass.Text = $"Last pass: {SharingWording.Describe(status)}";

        ShowAccounts(now);
    }

    /// <summary>
    /// The accounts the server will take, and which one this machine belongs
    /// to. The list is rebuilt only when the names change, because this
    /// redraws on every pass and rebuilding it under somebody who is choosing
    /// would drag the selection back to where they started.
    /// </summary>
    private void ShowAccounts(SyncState now)
    {
        var names = SharingWording.Options(now.Held, now.Account);
        if (!names.SequenceEqual(serverAccount.Items.OfType<string>(), StringComparer.Ordinal))
        {
            // What was on screen wins over what was saved: the server
            // answering mid-choice is what rebuilds this, and it must not
            // undo a choice.
            var showing = (serverAccount.Text ?? "").Trim();
            serverAccount.Items.Clear();
            foreach (var name in names)
            {
                serverAccount.Items.Add(name);
            }
            if (SharingWording.Selected(names, showing.Length == 0 ? now.Account : showing) is { } index)
            {
                serverAccount.SelectedIndex = index;
            }
            else
            {
                serverAccount.Text = showing;
            }
        }
        if (now.Held is { } held)
        {
            var sizes = string.Join(" · ", held.Select(entry => string.Create(CultureInfo.InvariantCulture, $"{entry.Name}: {entry.Rows:N0} rows")));
            ToolTipService.SetToolTip(serverAccount, sizes);
        }
    }

    /// <summary>The port of the GTK's confirm_forget_account: the name typed back before anything goes.</summary>
    private async Task ConfirmForget(string name)
    {
        var mine = account.Settings.SyncAccount.Trim();
        var isMine = name == mine || (mine.Length == 0 && name == "default");
        var detail = $"“{name}” and everything in it will be removed from the server — every evening, every collection, every counter it holds. There is no undo, and the server has no second copy.";
        var gap = Environment.NewLine + Environment.NewLine;
        detail += isMine
            ? gap + "This is the account this machine syncs to. Nothing here is deleted: the whole account stays on this machine, and Send Again would put it back on the server."
            : gap + "This is not the account this machine syncs to. Whatever holds it locally keeps its copy; nothing else does.";
        var typed = new TextBox { Header = $"Type {name} to confirm", Margin = new Thickness(0, 12, 0, 0) };
        var lines = new StackPanel();
        lines.Children.Add(new TextBlock { Text = detail, TextWrapping = TextWrapping.Wrap });
        lines.Children.Add(typed);
        var confirm = new ContentDialog
        {
            Title = $"Delete “{name}”?",
            Content = lines,
            PrimaryButtonText = "Delete",
            CloseButtonText = "Cancel",
            DefaultButton = ContentDialogButton.Close,
            IsPrimaryButtonEnabled = false,
            XamlRoot = XamlRoot,
        };
        typed.TextChanged += (_, _) => confirm.IsPrimaryButtonEnabled = typed.Text.Trim() == name;
        Hide();
        if (await confirm.ShowAsync() == ContentDialogResult.Primary)
        {
            var gone = await account.ForgetAccount(name);
            state.Text = gone.IsOk ? $"Deleted “{name}” on the server." : $"Could not delete “{name}”: {gone.Error}";
        }
        await ShowAsync();
        if (state.Text.StartsWith("Deleted", StringComparison.Ordinal))
        {
            return;
        }
        await Redraw();
    }

    private async Task ConfirmResend()
    {
        var confirm = new ContentDialog
        {
            Title = "Send everything again?",
            Content = new TextBlock
            {
                TextWrapping = TextWrapping.Wrap,
                Text = "Armory will forget what the server has been told and offer this whole account up from scratch. Nothing here is deleted. Do this after emptying the server for a different Battle.net account — otherwise the two would merge into one.",
            },
            PrimaryButtonText = "Send Again",
            CloseButtonText = "Cancel",
            DefaultButton = ContentDialogButton.Close,
            XamlRoot = XamlRoot,
        };
        Hide();
        if (await confirm.ShowAsync() == ContentDialogResult.Primary)
        {
            await account.Resend();
        }
        await ShowAsync();
    }
}
