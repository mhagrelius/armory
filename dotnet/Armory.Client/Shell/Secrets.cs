namespace Armory.Client.Shell;

/// <summary>
/// The operating system's credential store, behind the one interface this
/// shell has for it. The Windows app answers it with the Password Vault; a
/// test answers it with a dictionary. Secrets never go in
/// <see cref="Armory.Settings.Settings"/>, and the names below are the same
/// four the GTK build keeps in the keyring.
/// </summary>
public interface ISecrets
{
    string? Get(string name);

    void Set(string name, string value);

    void Clear(string name);
}

public static class SecretNames
{
    /// <summary>The attribute every secret is filed under. The application id, not "armory".</summary>
    public const string Resource = "us.hagreli.Armory";

    public const string ClientSecret = "battlenet-client-secret";

    public const string AccessToken = "battlenet-access-token";

    public const string RefreshToken = "battlenet-refresh-token";

    public const string SyncToken = "sync-token";
}

/// <summary>Secrets held in memory only. For tests, and for a session that has no vault.</summary>
public sealed class MemorySecrets : ISecrets
{
    private readonly Dictionary<string, string> held = new(StringComparer.Ordinal);

    public string? Get(string name) => held.GetValueOrDefault(name);

    public void Set(string name, string value) => held[name] = value;

    public void Clear(string name) => held.Remove(name);
}
