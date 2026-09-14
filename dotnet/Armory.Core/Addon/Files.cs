namespace Armory.Addon;

/// <summary>
/// Where the companion addon's files live under a WoW install.
/// </summary>
/// <remarks>
/// An addon cannot open a socket and cannot read a file. SavedVariables is
/// the only channel out of the game and writing Lua into the addon's folder
/// before the game loads is the only channel in. Both are files, and both are
/// addressed here.
/// </remarks>
public static class Files
{
    /// <summary>The account-wide SavedVariables file: <c>WTF/Account/ACCOUNT/SavedVariables/Addon.lua</c>.</summary>
    public static string AccountSavedVariables(string wowPath, string account, string addon) =>
        Path.Combine(wowPath, "WTF", "Account", account, "SavedVariables", $"{addon}.lua");

    /// <summary>A character's own SavedVariables file.</summary>
    public static string CharacterSavedVariables(string wowPath, string account, string realm, string character, string addon) =>
        Path.Combine(wowPath, "WTF", "Account", account, realm, character, "SavedVariables", $"{addon}.lua");

    /// <summary>
    /// The account folders under a WoW install. There is usually one, and it
    /// is usually the Battle.net account name in capitals. <c>SavedVariables</c>
    /// sits alongside them at the same level, so it is skipped rather than
    /// mistaken for an account.
    /// </summary>
    public static List<string> Accounts(string wowPath)
    {
        var root = Path.Combine(wowPath, "WTF", "Account");
        if (!Directory.Exists(root))
        {
            return [];
        }
        var accounts = Directory.EnumerateDirectories(root)
            .Select(Path.GetFileName)
            .OfType<string>()
            .Where(name => name != "SavedVariables")
            .ToList();
        accounts.Sort(StringComparer.Ordinal);
        return accounts;
    }

    /// <summary>Where the addon itself is installed.</summary>
    public static string AddonDirectory(string wowPath, string addon) =>
        Path.Combine(wowPath, "Interface", "AddOns", addon);

    /// <summary>
    /// Whether the collector addon is installed. A directory an addon manager
    /// half-created is not an installed addon, and treating it as one means
    /// silently waiting for a file that will never be written.
    /// </summary>
    public static bool IsInstalled(string wowPath, string addon) =>
        File.Exists(Path.Combine(AddonDirectory(wowPath, addon), $"{addon}.toc"));

    /// <summary>
    /// Every per-character SavedVariables file an addon has written. The
    /// layout is <c>WTF/Account/ACCOUNT/Realm/Character/SavedVariables/</c>,
    /// so this is a two-level walk. It is how the roster gets built with no
    /// web API at all. Characters never logged in on are simply absent, which
    /// is the trade the API would otherwise solve.
    /// </summary>
    public static List<string> CharacterFiles(string wowPath, string account, string addon)
    {
        var root = Path.Combine(wowPath, "WTF", "Account", account);
        if (!Directory.Exists(root))
        {
            return [];
        }
        var files = new List<string>();
        foreach (var realm in Directory.EnumerateDirectories(root))
        {
            // `SavedVariables` sits alongside the realm folders at this level
            // and is not one.
            if (Path.GetFileName(realm) == "SavedVariables")
            {
                continue;
            }
            foreach (var character in Directory.EnumerateDirectories(realm))
            {
                var file = Path.Combine(character, "SavedVariables", $"{addon}.lua");
                if (File.Exists(file))
                {
                    files.Add(file);
                }
            }
        }
        files.Sort(StringComparer.Ordinal);
        return files;
    }
}
