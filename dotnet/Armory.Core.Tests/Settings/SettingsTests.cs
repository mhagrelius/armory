using Armory.Blizzard;
using Xunit;

namespace Armory.Tests.Settings;

/// <summary>Ported from <c>core/src/settings.rs</c>.</summary>
public sealed class SettingsTests
{
    [Fact(DisplayName = "a_fresh_install_is_unregistered_and_defaults_to_the_americas")]
    public void A_fresh_install_is_unregistered_and_defaults_to_the_americas()
    {
        var settings = new Armory.Settings.Settings();
        Assert.False(settings.IsRegistered);
        Assert.Equal(Region.Us, settings.Region);
        Assert.True(settings.JournalAutomatic);
    }

    [Fact(DisplayName = "settings_round_trip_through_a_directory_that_did_not_exist")]
    public void Settings_round_trip_through_a_directory_that_did_not_exist()
    {
        var directory = Directory.CreateTempSubdirectory("armory-settings-");
        try
        {
            var path = Path.Combine(directory.FullName, "nested", "settings.json");
            var settings = new Armory.Settings.Settings
            {
                Region = Region.Eu,
                ClientId = "abc123",
                WowPath = "/games/wow",
                WowAccount = "PLAYER1",
                AddonOnly = false,
                JournalAutomatic = false,
                JournalServer = "http://127.0.0.1:9090",
                JournalBackend = Armory.Settings.JournalBackend.ClaudeCode,
                JournalModel = "opus",
                SyncUrl = "http://nas.example:8084",
                SyncAccount = "PLAYER1",
            };
            Assert.True(settings.Save(path).IsOk);
            Assert.Equal(settings, Armory.Settings.Settings.Load(path));
            // The backend is written as the Rust's serde code, not the enum's name.
            Assert.Contains("\"journal_backend\": \"claude-code\"", File.ReadAllText(path), StringComparison.Ordinal);
        }
        finally
        {
            directory.Delete(recursive: true);
        }
    }

    [Fact(DisplayName = "an_unreadable_settings_file_falls_back_rather_than_stopping_the_application")]
    public void An_unreadable_settings_file_falls_back_rather_than_stopping_the_application()
    {
        var directory = Directory.CreateTempSubdirectory("armory-settings-");
        try
        {
            var path = Path.Combine(directory.FullName, "settings.json");
            File.WriteAllText(path, "{ not json");
            Assert.Equal(new Armory.Settings.Settings(), Armory.Settings.Settings.Load(path));
        }
        finally
        {
            directory.Delete(recursive: true);
        }
    }

    [Fact(DisplayName = "a_missing_settings_file_is_simply_defaults")]
    public void A_missing_settings_file_is_simply_defaults()
    {
        Assert.Equal(new Armory.Settings.Settings(), Armory.Settings.Settings.Load(Path.Combine(Path.GetTempPath(), "nonexistent-" + Guid.NewGuid(), "settings.json")));
    }

    [Fact(DisplayName = "no_secret_has_a_field_to_be_written_into")]
    public void No_secret_has_a_field_to_be_written_into()
    {
        // The vault holds the Battle.net client secret and the sync token.
        var json = new Armory.Settings.Settings().ToJson();
        foreach (var banned in new[] { "secret", "api_key", "apikey", "token" })
        {
            Assert.DoesNotContain(banned, json, StringComparison.OrdinalIgnoreCase);
        }
    }

    [Fact(DisplayName = "the file the Rust writes reads here")]
    public void The_file_the_rust_writes_reads_here()
    {
        // Copying settings.json from the Linux machine is a reasonable thing
        // to do, and the Rust's serde output is this.
        var directory = Directory.CreateTempSubdirectory("armory-settings-");
        try
        {
            var path = Path.Combine(directory.FullName, "settings.json");
            File.WriteAllText(path, """{"region":"eu","client_id":"abc","wow_path":"/games/wow","wow_account":null,"addon_only":true,"journal_automatic":true,"journal_server":"http://127.0.0.1:8080","sync_url":"http://nas.example.ts.net:8084","sync_account":"default"}""");
            var settings = Armory.Settings.Settings.Load(path);
            Assert.Equal(Region.Eu, settings.Region);
            Assert.True(settings.AddonOnly);
            Assert.Equal("http://nas.example.ts.net:8084", settings.SyncUrl);
            // A file from before the journal had a choice of writer keeps the llama-server.
            Assert.Equal(Armory.Settings.JournalBackend.LlamaServer, settings.JournalBackend);
            Assert.Equal("sonnet", settings.JournalModel);
        }
        finally
        {
            directory.Delete(recursive: true);
        }
    }

    [Fact(DisplayName = "a_wow_install_is_found_by_its_wtf_folder")]
    public void A_wow_install_is_found_by_its_wtf_folder()
    {
        // The marker is WTF rather than the directory itself: an empty
        // `_retail_` left behind by an uninstall would otherwise match.
        var home = Directory.CreateTempSubdirectory("armory-home-");
        try
        {
            var install = Path.Combine(home.FullName, "Games", "World of Warcraft", "_retail_");
            var candidates = new[] { install };
            Assert.Null(Armory.Settings.Settings.FindWow(home.FullName, candidates));
            Directory.CreateDirectory(Path.Combine(install, "WTF"));
            Assert.Equal(install, Armory.Settings.Settings.FindWow(home.FullName, candidates));
        }
        finally
        {
            home.Delete(recursive: true);
        }
    }
}
