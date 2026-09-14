using System.Text.Json;
using System.Text.Json.Serialization;
using Armory.Blizzard;

namespace Armory.Settings;

/// <summary>
/// What the application remembers between runs, other than data.
/// </summary>
/// <remarks>
/// Not the client secret and not the sync token. Those go to the credential
/// vault, and keeping them out of this record is what stops them being
/// written to a file by accident when something new gets added here later.
/// The client id does live here: it travels in the authorize URL in plain
/// sight. The JSON is the Rust <c>Settings</c>'s, field for field, so a
/// settings file copied from the Linux machine reads here.
/// </remarks>
public sealed record Settings
{
    /// <summary>Where the journal's llama-server is unless said otherwise.</summary>
    public const string DefaultJournalServer = "http://127.0.0.1:8080";

    /// <summary>The Claude Code alias asked for unless said otherwise. Plenty for prose from supplied facts, and the quicker of the two.</summary>
    public const string DefaultJournalModel = "sonnet";

    [JsonPropertyName("region")]
    public Region Region { get; init; } = Region.Us;

    /// <summary>The API client the user registered. Empty until they have.</summary>
    [JsonPropertyName("client_id")]
    public string ClientId { get; init; } = "";

    /// <summary>Where WoW is installed, once found or chosen.</summary>
    [JsonPropertyName("wow_path")]
    public string? WowPath { get; init; }

    /// <summary>
    /// Which <c>WTF/Account/NAME</c> folder to read, when the install has more
    /// than one. A second appears when a second Battle.net login has used the
    /// same install, and reading the wrong one is somebody else's account.
    /// </summary>
    [JsonPropertyName("wow_account")]
    public string? WowAccount { get; init; }

    /// <summary>
    /// Whether the person has chosen to go without a Battle.net client.
    /// Separate from the client id being empty: empty is "not set up yet" and
    /// this is "set up, deliberately, without one".
    /// </summary>
    [JsonPropertyName("addon_only")]
    public bool AddonOnly { get; init; }

    /// <summary>
    /// Whether a new evening gets written up without being asked. On: entries
    /// are written by a llama-server on this machine, nothing is billed and
    /// nothing leaves, and a journal you have to remember to write is a
    /// journal that does not get written.
    /// </summary>
    [JsonPropertyName("journal_automatic")]
    public bool JournalAutomatic { get; init; } = true;

    /// <summary>Where that server is. An address, not a credential.</summary>
    [JsonPropertyName("journal_server")]
    public string JournalServer { get; init; } = DefaultJournalServer;

    /// <summary>
    /// What writes the entries: the llama-server at that address, or the
    /// Claude Code command-line signed in on this machine. The second is
    /// this person's own subscription, used through Anthropic's own tool,
    /// and it holds no credential of its own here: the CLI keeps its login
    /// and Armory only asks whether there is one.
    /// </summary>
    [JsonPropertyName("journal_backend")]
    public JournalBackend JournalBackend { get; init; } = JournalBackend.LlamaServer;

    /// <summary>
    /// Which model Claude Code is asked for, as the CLI names them: an alias
    /// such as <c>sonnet</c> or <c>opus</c>, or a full model id. Ignored by
    /// a llama-server, which serves whatever it was launched with.
    /// </summary>
    [JsonPropertyName("journal_model")]
    public string JournalModel { get; init; } = DefaultJournalModel;

    /// <summary>
    /// Where this account is shared to, if it is. Empty means Armory keeps to
    /// itself. The token is in the vault beside the Battle.net secret, and it
    /// is both or neither: half of it silently is an installation that looks
    /// configured and never syncs.
    /// </summary>
    [JsonPropertyName("sync_url")]
    public string SyncUrl { get; init; } = "";

    /// <summary>
    /// Which account on that server this machine belongs to. Empty means
    /// <c>default</c>. Letters, digits, dash, underscore and dot: the server
    /// refuses anything else, because this name becomes a directory on it.
    /// </summary>
    [JsonPropertyName("sync_account")]
    public string SyncAccount { get; init; } = "";

    private static JsonSerializerOptions Json { get; } = new(JsonSerializerDefaults.General)
    {
        WriteIndented = true,
        UnmappedMemberHandling = JsonUnmappedMemberHandling.Disallow,
    };

    /// <summary>Whether onboarding has got far enough to sign in.</summary>
    [JsonIgnore]
    public bool IsRegistered => ClientId.Trim().Length > 0;

    /// <summary>
    /// Read settings, falling back to defaults for anything missing. A file
    /// that will not parse is not worth stopping for: the application is
    /// usable with defaults, and refusing to start because one key went bad
    /// is worse than losing a preference.
    /// </summary>
    public static Settings Load(string path)
    {
        try
        {
            return JsonSerializer.Deserialize<Settings>(File.ReadAllBytes(path), Json) ?? new Settings();
        }
        catch (Exception error) when (error is IOException or UnauthorizedAccessException or JsonException)
        {
            return new Settings();
        }
    }

    public Result<Unit, string> Save(string path)
    {
        try
        {
            if (Path.GetDirectoryName(path) is { Length: > 0 } parent)
            {
                Directory.CreateDirectory(parent);
            }
            File.WriteAllBytes(path, JsonSerializer.SerializeToUtf8Bytes(this, Json));
            return Result<Unit, string>.Ok(Unit.Value);
        }
        catch (Exception error) when (error is IOException or UnauthorizedAccessException)
        {
            return Result<Unit, string>.Err(error.Message);
        }
    }

    /// <summary>The JSON, for the test that no secret has a field to be written into.</summary>
    public string ToJson() => JsonSerializer.Serialize(this, Json);

    private static readonly Environment.SpecialFolder[] ProgramFolders =
    [
        Environment.SpecialFolder.ProgramFilesX86,
        Environment.SpecialFolder.ProgramFiles,
    ];

    /// <summary>The layouts the standalone Battle.net prefix, Lutris, Steam's compatdata and Bottles produce.</summary>
    private static readonly string[] Prefixes =
    [
        "Games/battlenet/compatdata/pfx",
        "Games/battle-net/compatdata/pfx",
        "Games/battlenet/pfx",
        "Games/wow/pfx",
        ".wine",
        ".local/share/lutris/prefixes/battlenet",
        ".var/app/com.usebottles.bottles/data/bottles/bottles/battlenet",
    ];

    /// <summary>
    /// Everywhere a WoW retail install might be on this machine. On Windows
    /// the launcher's default and both Program Files; elsewhere the Wine and
    /// Proton prefixes the Rust build looks in, the path inside a prefix being
    /// always the same.
    /// </summary>
    public static List<string> WowSearchPaths(string home)
    {
        const string retail = "World of Warcraft/_retail_";
        if (OperatingSystem.IsWindows())
        {
            var candidates = new List<string>();
            foreach (var folder in ProgramFolders)
            {
                var programs = Environment.GetFolderPath(folder);
                if (programs.Length > 0)
                {
                    candidates.Add(Path.Combine(programs, "World of Warcraft", "_retail_"));
                }
            }
            candidates.Add(Path.Combine(Path.GetPathRoot(home) ?? "C:\\", "Games", "World of Warcraft", "_retail_"));
            return candidates.Distinct(StringComparer.OrdinalIgnoreCase).ToList();
        }
        const string insidePrefix = "drive_c/Program Files (x86)/" + retail;
        return Prefixes.Select(prefix => Path.Combine(home, prefix, insidePrefix)).ToList();
    }

    /// <summary>
    /// The first search path that actually holds a <c>WTF</c> folder. The
    /// marker is WTF rather than the directory itself: an empty install left
    /// behind by an uninstall would otherwise match.
    /// </summary>
    public static string? FindWow(string home, IEnumerable<string>? candidates = null) =>
        (candidates ?? WowSearchPaths(home)).FirstOrDefault(path => Directory.Exists(Path.Combine(path, "WTF")));
}

/// <summary>What writes a journal entry. The JSON codes are the Rust <c>Settings</c>'s, kebab-case.</summary>
[JsonConverter(typeof(JournalBackendConverter))]
public enum JournalBackend
{
    /// <summary>llama.cpp's server on this machine, at <see cref="Settings.JournalServer"/>.</summary>
    LlamaServer,

    /// <summary>The <c>claude</c> command-line, signed in to this person's own subscription.</summary>
    ClaudeCode,
}

public static class JournalBackendExtensions
{
    public static string Code(this JournalBackend backend) => backend switch
    {
        JournalBackend.ClaudeCode => "claude-code",
        _ => "llama-server",
    };

    public static JournalBackend? FromCode(string code) => code switch
    {
        "llama-server" => JournalBackend.LlamaServer,
        "claude-code" => JournalBackend.ClaudeCode,
        _ => null,
    };
}

public sealed class JournalBackendConverter : JsonConverter<JournalBackend>
{
    public override JournalBackend Read(ref Utf8JsonReader reader, Type typeToConvert, JsonSerializerOptions options) =>
        JournalBackendExtensions.FromCode(reader.GetString() ?? "") ?? throw new JsonException("not a journal backend");

    public override void Write(Utf8JsonWriter writer, JournalBackend value, JsonSerializerOptions options) =>
        writer.WriteStringValue(value.Code());
}
