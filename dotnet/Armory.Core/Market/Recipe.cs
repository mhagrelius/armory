using Armory.Roster;

namespace Armory.Market;

/// <summary>
/// One thing a character knows how to make. The crafting tally says what
/// somebody has made; this says what they can make, which is the question a
/// flip needs. Neither has an endpoint.
/// </summary>
public sealed record Recipe
{
    /// <summary>The recipe's spell id, which is what the game keys it by.</summary>
    public long Id { get; init; }

    public string Name { get; init; } = "";

    /// <summary>The item the craft produces. Recipes with no output item are not recorded: there is no price to look up.</summary>
    public long Output { get; init; }

    /// <summary>How many it makes at minimum. Never the maximum: costing a flip against the lucky outcome is how a margin becomes fiction.</summary>
    public long Makes { get; init; } = 1;

    public List<Reagent> Reagents { get; init; } = [];

    public bool Equals(Recipe? other) =>
        other is not null && Id == other.Id && Name == other.Name && Output == other.Output && Makes == other.Makes && Reagents.SequenceEqual(other.Reagents);

    public override int GetHashCode() => HashCode.Combine(Id, Name, Output, Makes);
}

/// <summary>
/// One required reagent slot, and every quality it can be filled with. The
/// tiers are separate item ids rather than variants of one, which the auction
/// house proves: reagents are commodities and a commodity carries no bonus
/// ids to vary by.
/// </summary>
public sealed record Reagent
{
    public long Quantity { get; init; } = 1;

    public List<long> Tiers { get; init; } = [];

    public bool Equals(Reagent? other) => other is not null && Quantity == other.Quantity && Tiers.SequenceEqual(other.Tiers);

    public override int GetHashCode() => HashCode.Combine(Quantity, Tiers.Count);
}

/// <summary>What every character can make, keyed by who can make it.</summary>
public sealed class RecipeBooks : Dictionary<CharacterKey, List<Recipe>>
{
}
