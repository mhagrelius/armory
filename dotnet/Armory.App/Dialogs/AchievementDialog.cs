using System.Globalization;
using Armory.App.Pages;
using Armory.Blizzard;
using Armory.Client.Shell;
using Armory.Roster;
using Armory.Run;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;

namespace Armory.App.Dialogs;

/// <summary>
/// One goal: where it stands, how it is being measured and what that
/// costs, somewhere to read more, and the two decisions a person can make
/// about it. The port of <c>ui/achievement_dialog.rs</c>; undesigned in the
/// handoff, so it is a plain Fluent content dialog.
/// </summary>
public static class AchievementDialog
{
    public static async Task Show(XamlRoot root, Account account, Goal goal, Achievement? achievement)
    {
        var column = new StackPanel { Spacing = 18 };
        column.Children.Add(Heading(goal, achievement));
        column.Children.Add(StandingGroup(goal));
        column.Children.Add(Tracking(goal));
        column.Children.Add(Links(goal, achievement));

        var dialog = new ContentDialog
        {
            Title = achievement?.Name is { Length: > 0 } name ? name : "Achievement",
            Content = new ScrollViewer { Content = column, MaxHeight = 640 },
            CloseButtonText = "Close",
            DefaultButton = ContentDialogButton.Close,
            XamlRoot = root,
        };
        if (goal.Standing.IsPoisoned && Decisions(account, goal, dialog) is { } decisions)
        {
            column.Children.Add(decisions);
        }
        await dialog.ShowAsync();
    }

    private static StackPanel Heading(Goal goal, Achievement? achievement)
    {
        var stack = new StackPanel { Spacing = 6 };
        stack.Children.Add(new TextBlock
        {
            Text = achievement?.Name is { Length: > 0 } name ? name : string.Create(CultureInfo.InvariantCulture, $"Achievement {goal.AchievementId}"),
            Style = (Style)Application.Current.Resources["Subtitle"],
            TextWrapping = TextWrapping.Wrap,
        });
        if (achievement is null)
        {
            return stack;
        }
        var parts = new List<string>();
        if (achievement.Category.Length > 0)
        {
            parts.Add(achievement.Category);
        }
        if (achievement.Points > 0)
        {
            parts.Add(string.Create(CultureInfo.InvariantCulture, $"{achievement.Points} points"));
        }
        if (parts.Count > 0)
        {
            stack.Children.Add(Widgets.Text(string.Join("  ·  ", parts), "Secondary"));
        }
        if (achievement.Description.Length > 0)
        {
            stack.Children.Add(Widgets.Text(achievement.Description));
        }
        return stack;
    }

    /// <summary>Why this is on the list, in words.</summary>
    private static StackPanel StandingGroup(Goal goal)
    {
        var (title, detail) = goal.Standing switch
        {
            Standing.Unearned => (
                "Nobody on the account has this",
                "So the game's own completion flag works normally. When an enrolled character earns it, it lights up and Armory reads it like any other tracker would."),
            Standing.EarnedDuringRun during => (
                "Earned during this run",
                $"Finished on {during.At.ToString("d MMMM yyyy", CultureInfo.InvariantCulture)}. Anything completed after the baseline belongs to the run — nobody else is playing this account."),
            Standing.EarnedByCohort cohort => (
                "An enrolled character earned this",
                $"{Who(cohort.By)} has it, and {Who(cohort.By)} is in this run. Nothing further needs computing."),
            Standing.Poisoned { By: { } by } => (
                "Earned before the run, by someone outside it",
                $"{Who(by)} earned this before the baseline was taken. The completion flag was set then and will never move again — a second character finishing the same content produces no signal at all. So Armory ignores the flag and works from each enrolled character's own data instead."),
            _ => (
                "Earned before the run, by an unknown character",
                "The account had this before the baseline and nothing records who earned it. Logging in on more of your characters fills this in: the game tells the collector addon which achievements the character you are playing earned, so each login attributes a few hundred more."),
        };
        var group = Group("Where this stands");
        group.Children.Add(Row(title, detail));
        return group;
    }

    /// <summary>How it is being measured, and what that costs.</summary>
    private static StackPanel Tracking(Goal goal)
    {
        var group = Group("How it is tracked");
        if (!goal.Standing.IsPoisoned)
        {
            group.Children.Add(Row("By the game's own flag", "Nothing here needs recomputing."));
            return group;
        }
        var (title, detail) = goal.Bucket switch
        {
            Bucket.Observable => (
                "Measured from your characters' own data",
                (goal.Evaluation is { } evaluation ? string.Create(CultureInfo.InvariantCulture, $"{evaluation.Progress} of {evaluation.Required} so far. ") : "")
                + "Every one of this achievement's criteria maps to something recorded per character — quests completed, encounters cleared — so progress is computed rather than guessed."),
            Bucket.Attestable => (
                "Only you can say",
                "At least one of this achievement's criteria is something WoW records account-wide only — a creature killed, an area explored, a spell cast. There is no per-character record to measure against, so rather than draw a progress bar over a number that means something else, Armory asks you."),
            Bucket.Excluded excluded => (
                "Left out of this run",
                excluded.Why switch
                {
                    Exclusion.AlreadyOwned => "The account already has what this awards, and it cannot be collected twice.",
                    Exclusion.Unrepeatable => "A Feat of Strength or legacy achievement. Nobody can earn this again, so leaving it in the backlog would leave a row that can only ever read zero.",
                    Exclusion.Unmeasurable => "Nothing measures it and nobody could honestly attest to it.",
                    _ => "You took this out of the run.",
                }),
            _ => ("Unknown", ""),
        };
        group.Children.Add(Row(title, detail));
        if (goal.Evaluation is { Inherited: true })
        {
            group.Children.Add(Row(
                "Some of this was inherited",
                "Part of the progress comes from a reputation The War Within made account-wide, which an unenrolled character may well have earned. It is shown but never counted."));
        }
        return group;
    }

    private static StackPanel Links(Goal goal, Achievement? achievement)
    {
        var group = Group("Read more", "Armory fetches nothing from these — Wowhead's terms forbid automated access — but they are where the criteria, comments and guides live.");
        var id = goal.AchievementId.ToString(CultureInfo.InvariantCulture);
        group.Children.Add(Link("Wowhead", "Criteria, comments and guides", $"https://www.wowhead.com/achievement={id}"));
        var search = achievement is { Name.Length: > 0 } ? Api.Encode(achievement.Name) : id;
        group.Children.Add(Link("Warcraft Wiki", "Community documentation", $"https://warcraft.wiki.gg/wiki/Special:Search?search={search}"));
        return group;
    }

    /// <summary>
    /// The two things a person can decide: that they did it, and that it is
    /// not part of this run. Both only for a poisoned goal, which is the
    /// only kind the game's own flag cannot answer.
    /// </summary>
    private static StackPanel? Decisions(Account account, Goal goal, ContentDialog dialog)
    {
        var group = Group("Your call");
        var id = goal.AchievementId;
        switch (goal.Bucket)
        {
            case Bucket.Attestable:
                var attested = goal.Attestation is not null;
                var toggle = new ToggleSwitch { Header = "I did this on an enrolled character", IsOn = attested, OnContent = "Attested", OffContent = "Not yet" };
                toggle.Toggled += async (sender, _) =>
                {
                    var on = ((ToggleSwitch)sender).IsOn;
                    if (on == attested)
                    {
                        return;
                    }
                    attested = on;
                    // The cohort's first member stands in for "me" until goals carry a character picker.
                    await account.Attest(id, on ? account.Cohort.Keys.FirstOrDefault() : null);
                };
                group.Children.Add(toggle);
                group.Children.Add(Widgets.Text("A person's word survives a replan.", "Caption"));
                break;
            case Bucket.Excluded { Why: Exclusion.ByHand }:
                var restore = Widgets.Standard("Put it back in the run");
                restore.Click += async (_, _) =>
                {
                    await account.SetExcluded(id, false);
                    dialog.Hide();
                };
                group.Children.Add(restore);
                break;
            case Bucket.Excluded:
                return null;
            default:
                var exclude = Widgets.Standard("Leave this out of the run");
                exclude.Click += async (_, _) =>
                {
                    await account.SetExcluded(id, true);
                    dialog.Hide();
                };
                group.Children.Add(exclude);
                group.Children.Add(Widgets.Text("An excluded goal is not done, it is gone. It stops counting against the run.", "Caption"));
                break;
        }
        return group;
    }

    private static string Who(CharacterKey key) => key.Name.Length > 0 ? char.ToUpperInvariant(key.Name[0]) + key.Name[1..] : key.RealmSlug;

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

    private static Border Row(string title, string subtitle)
    {
        var stack = new StackPanel { Spacing = 2 };
        stack.Children.Add(new TextBlock { Text = title, TextWrapping = TextWrapping.Wrap });
        stack.Children.Add(new TextBlock { Text = subtitle, Style = (Style)Application.Current.Resources["Caption"], TextWrapping = TextWrapping.Wrap });
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
