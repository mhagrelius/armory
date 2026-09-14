using Armory.Addon;
using Xunit;

namespace Armory.Tests.Addon;

/// <summary>Ported from <c>core/src/addon/mod.rs</c>. Real directories under a temp folder, no fakes.</summary>
public sealed class FilesTests
{
    private const string Addon = "Armory_Collector";

    [Fact(DisplayName = "saved_variables_land_where_the_client_writes_them")]
    public void Saved_variables_land_where_the_client_writes_them()
    {
        var wow = Path.Combine("C:", "games", "wow", "_retail_");
        Assert.Equal(
            Path.Combine(wow, "WTF", "Account", "PLAYER1", "SavedVariables", "Armory_Collector.lua"),
            Files.AccountSavedVariables(wow, "PLAYER1", Addon));
        Assert.Equal(
            Path.Combine(wow, "WTF", "Account", "PLAYER1", "Emerald Dream", "Somechar", "SavedVariables", "Armory_Collector.lua"),
            Files.CharacterSavedVariables(wow, "PLAYER1", "Emerald Dream", "Somechar", Addon));
    }

    [Fact(DisplayName = "the_shared_saved_variables_folder_is_not_an_account")]
    public void The_shared_saved_variables_folder_is_not_an_account()
    {
        // It sits at the same level as the account folders and would otherwise
        // be read as one, producing a phantom account with no characters.
        using var wow = new TempWow();
        Directory.CreateDirectory(Path.Combine(wow.Path, "WTF", "Account", "PLAYER1"));
        Directory.CreateDirectory(Path.Combine(wow.Path, "WTF", "Account", "SavedVariables"));

        Assert.Equal(["PLAYER1"], Files.Accounts(wow.Path));
    }

    [Fact(DisplayName = "an_install_with_no_wtf_folder_has_no_accounts_rather_than_failing")]
    public void An_install_with_no_wtf_folder_has_no_accounts_rather_than_failing()
    {
        Assert.Empty(Files.Accounts(Path.Combine(Path.GetTempPath(), "nonexistent-" + Guid.NewGuid())));
    }

    [Fact(DisplayName = "every_character_that_has_logged_in_leaves_a_file")]
    public void Every_character_that_has_logged_in_leaves_a_file()
    {
        // This is the roster, with no web API involved.
        using var wow = new TempWow();
        var account = Path.Combine(wow.Path, "WTF", "Account", "PLAYER1");
        foreach (var (realm, character) in new[] { ("Emerald Dream", "Somechar"), ("Emerald Dream", "Velkurai"), ("Mannoroth", "Aeltor") })
        {
            var directory = Path.Combine(account, realm, character, "SavedVariables");
            Directory.CreateDirectory(directory);
            File.WriteAllText(Path.Combine(directory, "Armory_Collector.lua"), "x");
        }
        // The account-wide folder sits at the same level as the realms, and a
        // walk that treats it as one produces a phantom realm.
        Directory.CreateDirectory(Path.Combine(account, "SavedVariables"));
        // A character who has the folder but not our file; every other addon
        // makes these.
        Directory.CreateDirectory(Path.Combine(account, "Thrall", "Ulahae", "SavedVariables"));

        var files = Files.CharacterFiles(wow.Path, "PLAYER1", Addon);
        Assert.Equal(3, files.Count);
        Assert.All(files, file => Assert.EndsWith("Armory_Collector.lua", file, StringComparison.Ordinal));
    }

    [Fact(DisplayName = "an_account_with_no_characters_yet_yields_nothing_rather_than_failing")]
    public void An_account_with_no_characters_yet_yields_nothing_rather_than_failing()
    {
        Assert.Empty(Files.CharacterFiles(Path.Combine(Path.GetTempPath(), "nonexistent-" + Guid.NewGuid()), "PLAYER1", Addon));
    }

    [Fact(DisplayName = "the_addon_counts_as_installed_only_once_its_toc_is_there")]
    public void The_addon_counts_as_installed_only_once_its_toc_is_there()
    {
        using var wow = new TempWow();
        var directory = Files.AddonDirectory(wow.Path, Addon);
        Directory.CreateDirectory(directory);
        Assert.False(Files.IsInstalled(wow.Path, Addon));

        File.WriteAllText(Path.Combine(directory, "Armory_Collector.toc"), "## Interface: 110200");
        Assert.True(Files.IsInstalled(wow.Path, Addon));
    }

    /// <summary>A throwaway WoW install root that removes itself.</summary>
    internal sealed class TempWow : IDisposable
    {
        public TempWow()
        {
            Path = Directory.CreateTempSubdirectory("armory-wow-").FullName;
        }

        public string Path { get; }

        public void Dispose() => Directory.Delete(Path, recursive: true);
    }
}
