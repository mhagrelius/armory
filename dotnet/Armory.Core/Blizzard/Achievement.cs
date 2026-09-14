using Armory.Run;

namespace Armory.Blizzard;

/// <summary>One achievement, as the catalogue describes it.</summary>
public sealed record Achievement
{
    public long Id { get; init; }

    public string Name { get; init; } = "";

    public string Category { get; init; } = "";

    public long Points { get; init; }

    public string Description { get; init; } = "";

    /// <summary>Feats of Strength and the like: never earnable again, and excluded from a run rather than left in it as permanent zeroes.</summary>
    public bool IsUnrepeatable { get; init; }
}

/// <summary>One achievement as the profile reports it: the criteria tree, and when, if ever, the account finished it.</summary>
public sealed record AchievementProgress
{
    public long Id { get; init; }

    public DateTimeOffset? CompletedAt { get; init; }

    public Criterion? Criteria { get; init; }
}
