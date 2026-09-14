using Armory.Blizzard;
using Armory.Client.Shell;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;

namespace Armory.App.Pages.Settings;

/// <summary>
/// The Battle.net card on Settings: which client and region this machine
/// uses, whether a session is held, and the way to sign in again, sign out,
/// or go back to setup. The full flow lives on the onboarding page; this is
/// the short form for somebody already set up.
/// </summary>
public static class BattleNetSection
{
    public static Border Build(Account account, MainWindow window)
    {
        var card = new StackPanel { Spacing = 10 };
        var registered = account.Settings.IsRegistered;
        var signedIn = account.Token is not null;
        card.Children.Add(Widgets.CardHead(
            "Battle.net",
            !registered ? (account.Settings.AddonOnly ? "addon only" : "not set up") : signedIn ? "signed in" : "signed out"));

        if (registered)
        {
            card.Children.Add(Widgets.Text($"Client {account.Settings.ClientId} · {account.Settings.Region.Label()}", "Secondary"));
            card.Children.Add(Widgets.Text(signedIn
                ? "The session lasts a day. Sync reads every enrolled character, the collections, the catalogue and the artwork."
                : account.StoredSecret() is null
                    ? "No secret is saved for this client, so signing in needs it pasted again."
                    : "The secret is still saved, so the sign-in button is all it takes.", "Caption"));
        }
        else
        {
            card.Children.Add(Widgets.Text(account.Settings.AddonOnly
                ? "Going without a Battle.net client. The addon supplies everything except the Market tab and alts you have never logged in on."
                : "Nothing set up yet.", "Caption"));
        }

        var row = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 8 };
        if (registered)
        {
            var sync = Widgets.Accent("Sync now", "");
            sync.IsEnabled = signedIn;
            sync.Click += async (_, _) => await account.Sync();
            row.Children.Add(sync);
            var signIn = Widgets.Standard(signedIn ? "Sign in again" : "Sign in");
            signIn.Click += (_, _) => window.Open("onboarding");
            row.Children.Add(signIn);
            var signOut = Widgets.Standard("Sign out");
            signOut.Click += (_, _) => account.SignOut();
            row.Children.Add(signOut);
            var art = Widgets.Standard("Fetch all artwork");
            art.IsEnabled = signedIn;
            art.Click += async (_, _) => await account.FetchAllArt();
            row.Children.Add(art);
        }
        else
        {
            var setUp = Widgets.Accent("Set up a client…");
            setUp.Click += (_, _) => window.Open("onboarding");
            row.Children.Add(setUp);
        }
        card.Children.Add(row);
        return Widgets.Card(card);
    }
}
