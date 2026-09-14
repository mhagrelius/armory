namespace Armory.Sharing;

/// <summary>
/// A table that travels.
/// </summary>
/// <remarks>
/// Twenty-seven of them: everything the store holds except <c>change</c>,
/// which is the log, and <c>sync_state</c>, which is the cursor beside it.
/// There is no opt-out list to keep in step with the schema: a new table is
/// either here or it does not travel, and the tests say so out loud.
/// </remarks>
public enum Scope
{
    Character,
    Enrolment,
    Detail,
    Attribution,
    Currency,
    EarnedReputation,
    EarnedCurrency,
    Tally,
    Recipe,
    RecipeReagent,
    Instance,
    Encounter,
    Criterion,
    WarbandItem,
    PetHeld,
    Run,
    Goal,
    Collectible,
    Achievement,
    Price,
    Snapshot,
    Item,
    Watched,
    WatchedRealm,
    Session,
    Entry,
    Forgotten,
    Response,
}

/// <summary>How one column settles when both sides have one.</summary>
public enum Rule
{
    /// <summary>The arriving value wins, subject to the row's guard.</summary>
    Take,

    /// <summary>
    /// The larger number wins, whichever side it is on. The counters no
    /// Blizzard system keeps: cumulative, and nowhere to get them back from,
    /// so a machine that has been away must not take them off one that is
    /// ahead. The one rule under which arrival order does not matter at all.
    /// </summary>
    Max,

    /// <summary>
    /// Deserialise both sides and merge them field by field. One column,
    /// <c>collectible.json</c>: the addon has the source prose and the art,
    /// the web API has the name and the expansion, and taking either whole
    /// takes the other's half off.
    /// </summary>
    MergeJson,

    /// <summary>A byte column, base64 on the wire. One column, <c>response.body</c>.</summary>
    Blob,

    /// <summary>
    /// Taken like <see cref="Take"/>, but never the reason a row travels.
    /// Exactly the columns a table is guarded on: the stamp a row is judged
    /// by is not itself news. Without this every guarded table re-sends
    /// itself on a timer.
    /// </summary>
    Stamp,
}

/// <summary>When an arriving row is allowed to land at all.</summary>
public enum GuardKind
{
    /// <summary>Always.</summary>
    Always,

    /// <summary>Only if the arriving row's stamp is at or past the held one's.</summary>
    Newer,

    /// <summary>Never overwrite. Written once, and what is held wins.</summary>
    Keep,
}

public readonly record struct Guard(GuardKind Kind, string? Column)
{
    public static Guard Always { get; } = new(GuardKind.Always, null);

    public static Guard Keep { get; } = new(GuardKind.Keep, null);

    public static Guard Newer(string column) => new(GuardKind.Newer, column);
}

/// <summary>What a column is, and what happens when both sides have one.</summary>
public sealed record Column(string Name, Rule Rule)
{
    public static Column Take(string name) => new(name, Rule.Take);

    public static Column Max(string name) => new(name, Rule.Max);

    public static Column Stamp(string name) => new(name, Rule.Stamp);
}

/// <summary>
/// Which key column, if any, is a local row id that means nothing on another
/// machine. One table: <c>goal.run_id</c>. On the wire the column carries
/// <c>run.key</c> instead and is translated back on the way in.
/// </summary>
public readonly record struct LocalId(int Position, Scope Scope);

/// <summary>One table, as the wire sees it.</summary>
public sealed record Table(
    Scope Scope,
    string Name,
    IReadOnlyList<string> Key,
    IReadOnlyList<Column> Columns,
    Guard Guard,
    LocalId? LocalId)
{
    /// <summary>Every column, key first, in the order a row is written.</summary>
    public IEnumerable<string> AllColumns => Key.Concat(Columns.Select(column => column.Name));
}

/// <summary>Every table that travels, in the order rows are applied.</summary>
public static class Tables
{
    private static readonly Dictionary<Scope, Table> ByScope;
    private static readonly Dictionary<string, Table> ByName;

    static Tables()
    {
        ByScope = All.ToDictionary(table => table.Scope);
        ByName = All.ToDictionary(table => table.Name, StringComparer.Ordinal);
    }

    public static IReadOnlyList<Table> All { get; } =
    [
        new(Scope.Character, "character", ["realm_slug", "name"],
            [
                Column.Take("character_id"),
                Column.Take("realm_id"),
                Column.Take("display_name"),
                Column.Take("realm_name"),
                Column.Take("level"),
                Column.Take("class"),
                Column.Take("race"),
                Column.Take("faction"),
                Column.Take("wow_account_id"),
            ],
            Guard.Always, null),
        new(Scope.Enrolment, "enrolment", ["realm_slug", "name"], [], Guard.Always, null),
        new(Scope.Detail, "detail", ["realm_slug", "name"],
            [Column.Take("json"), Column.Stamp("fetched_at")],
            Guard.Newer("fetched_at"), null),
        new(Scope.Attribution, "attribution", ["achievement_id"],
            [Column.Take("realm_slug"), Column.Take("name")],
            Guard.Always, null),
        new(Scope.Currency, "currency", ["realm_slug", "name", "currency_id"],
            [Column.Take("amount")],
            Guard.Always, null),
        new(Scope.EarnedReputation, "earned_reputation", ["realm_slug", "name", "faction_id"],
            [
                Column.Max("points"),
                Column.Max("renown"),
                Column.Max("renown_seen"),
                Column.Take("account_wide"),
            ],
            Guard.Always, null),
        new(Scope.EarnedCurrency, "earned_currency", ["realm_slug", "name", "currency_id"],
            [
                Column.Max("gained"),
                Column.Max("earned"),
                Column.Take("tracks_earned"),
                Column.Take("account_wide"),
                Column.Take("transferable"),
            ],
            Guard.Always, null),
        new(Scope.Tally, "tally", ["realm_slug", "name", "kind", "key"],
            [Column.Max("count"), Column.Take("label")],
            Guard.Always, null),
        new(Scope.Recipe, "recipe", ["realm_slug", "name", "recipe_id"],
            [Column.Take("recipe"), Column.Take("output_id"), Column.Take("makes")],
            Guard.Always, null),
        new(Scope.RecipeReagent, "recipe_reagent", ["realm_slug", "name", "recipe_id", "slot"],
            [Column.Take("quantity"), Column.Take("tiers")],
            Guard.Always, null),
        new(Scope.Instance, "instance", ["id"],
            [
                Column.Take("name"),
                Column.Take("map"),
                Column.Take("description"),
                Column.Take("expansion"),
                Column.Take("encounters"),
            ],
            Guard.Always, null),
        new(Scope.Encounter, "encounter", ["id"],
            [Column.Take("name"), Column.Take("description"), Column.Take("loot")],
            Guard.Always, null),
        new(Scope.Criterion, "criterion", ["criterion_id"], [Column.Take("kind")], Guard.Always, null),
        new(Scope.WarbandItem, "warband_item", ["item_id"], [Column.Take("count")], Guard.Always, null),
        new(Scope.PetHeld, "pet_held", ["species_id"], [Column.Take("count")], Guard.Always, null),
        // Before `goal`, always: a goal names its run and is dropped if the
        // run is not here. Rows travel in `seq` order and the run is written
        // first, so this is the order they arrive in as well.
        new(Scope.Run, "run", ["key"],
            [
                Column.Take("name"),
                Column.Take("baseline"),
                Column.Take("cohort"),
                Column.Take("is_current"),
            ],
            Guard.Always, null),
        new(Scope.Goal, "goal", ["run_id", "achievement_id"],
            [Column.Take("standing"), Column.Take("bucket"), Column.Take("attestation")],
            Guard.Always, new LocalId(0, Scope.Run)),
        new(Scope.Collectible, "collectible", ["kind", "id"],
            [new Column("json", Rule.MergeJson), Column.Max("owned")],
            Guard.Always, null),
        new(Scope.Achievement, "achievement", ["id"],
            [
                Column.Take("name"),
                Column.Take("category"),
                Column.Take("points"),
                Column.Take("description"),
                Column.Take("unrepeatable"),
            ],
            Guard.Always, null),
        new(Scope.Price, "price", ["realm", "item_id", "variant", "seen_at"],
            [
                Column.Take("unit_price"),
                Column.Take("quantity"),
                Column.Take("listings"),
                Column.Take("tenth"),
                Column.Take("median"),
            ],
            Guard.Keep, null),
        new(Scope.Snapshot, "snapshot", ["realm", "item_id", "variant"],
            [
                Column.Take("cheapest"),
                Column.Take("quantity"),
                Column.Take("listings"),
                Column.Take("tenth"),
                Column.Take("median"),
                Column.Stamp("seen_at"),
            ],
            Guard.Newer("seen_at"), null),
        new(Scope.Item, "item", ["item_id"],
            [Column.Take("name"), Column.Take("sellable"), Column.Take("quality")],
            Guard.Always, null),
        new(Scope.Watched, "watched", ["item_id"], [Column.Take("name")], Guard.Always, null),
        new(Scope.WatchedRealm, "watched_realm", ["realm_id"], [Column.Take("name")], Guard.Always, null),
        new(Scope.Session, "session", ["realm_slug", "name", "started_at"],
            [Column.Take("ended_at"), Column.Take("json")],
            Guard.Keep, null),
        new(Scope.Entry, "entry", ["realm_slug", "name", "started_at"],
            [
                Column.Take("title"),
                Column.Take("body"),
                Column.Take("model"),
                Column.Stamp("written_at"),
            ],
            Guard.Newer("written_at"), null),
        new(Scope.Forgotten, "forgotten", ["realm_slug", "name", "started_at"],
            [Column.Take("at")],
            Guard.Always, null),
        new(Scope.Response, "response", ["url"],
            [
                new Column("body", Rule.Blob),
                Column.Take("last_modified"),
                Column.Stamp("fetched_at"),
            ],
            Guard.Newer("fetched_at"), null),
    ];

    /// <summary>The description of this scope's table.</summary>
    public static Table Of(Scope scope) => ByScope[scope];

    /// <summary>
    /// The scope a wire name refers to, if this build knows it. Null for a
    /// scope a newer build sends and this one has no table for: the row is
    /// counted and dropped rather than guessed at.
    /// </summary>
    public static Scope? Named(string name) =>
        ByName.TryGetValue(name, out var table) ? table.Scope : null;

    public static string NameOf(Scope scope) => Of(scope).Name;
}
