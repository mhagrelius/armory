namespace Armory.Client.Shell;

/// <summary>Where this installation keeps its files. The Windows equivalents of the XDG directories the GTK build uses.</summary>
public static class Paths
{
    /// <summary>Settings: roaming, because they are small and a person's choices.</summary>
    public static string ConfigDir =>
        Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.ApplicationData), "Armory");

    /// <summary>The store and the image cache: local, because they are large and re-derivable.</summary>
    public static string DataDir =>
        Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "Armory");

    public static string SettingsPath => Path.Combine(ConfigDir, "settings.json");

    public static string StorePath => Path.Combine(DataDir, "armory.db");

    public static string CachePath => Path.Combine(DataDir, "cache");

    /// <summary>
    /// Where the journal's command-line is started. Empty on purpose: the CLI
    /// reads instructions from the directory it runs in, and this one holds
    /// none.
    /// </summary>
    public static string JournalDir => Path.Combine(DataDir, "journal");

    /// <summary>Where an unhandled error is written, since a Windows app has no stderr anybody sees.</summary>
    public static string LogPath => Path.Combine(DataDir, "armory.log");
}
