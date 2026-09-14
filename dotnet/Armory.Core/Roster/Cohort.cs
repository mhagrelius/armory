using System.Text.Json.Serialization;

namespace Armory.Roster;

/// <summary>
/// Which characters the application is actually about.
/// </summary>
/// <remarks>
/// Every character on the account is synced, but sync is not enrolment.
/// Enrolment is explicit and per character. The cohort is what the run is
/// measured against and what the interface shows. Everyone else stays in the
/// database for exactly one purpose: explaining why something is already
/// owned. When a mount cannot be collected again because Aeltor looted it in
/// 2016, Aeltor has to still be there to say so.
/// </remarks>
public sealed record Cohort
{
    public Cohort()
    {
    }

    public Cohort(IEnumerable<CharacterKey> keys)
    {
        foreach (var key in keys)
        {
            Members.Add(key);
        }
    }

    [JsonPropertyName("members")]
    public SortedSet<CharacterKey> Members { get; init; } = [];

    public bool Contains(CharacterKey key) => Members.Contains(key);

    public void Enrol(CharacterKey key) => Members.Add(key);

    public void Withdraw(CharacterKey key) => Members.Remove(key);

    /// <summary>Enrol or withdraw, and report what the state became.</summary>
    public bool Toggle(CharacterKey key)
    {
        if (Contains(key))
        {
            Withdraw(key);
            return false;
        }
        Enrol(key);
        return true;
    }

    [JsonIgnore]
    public int Count => Members.Count;

    [JsonIgnore]
    public bool IsEmpty => Members.Count == 0;

    [JsonIgnore]
    public IEnumerable<CharacterKey> Keys => Members;

    /// <summary>The enrolled characters, in roster order.</summary>
    public List<Character> MembersOf(Roster roster) =>
        roster.Characters.Where(character => Contains(character.Key)).ToList();

    /// <summary>The rest of the account: not shown, not measured, kept only to explain why something is already spent.</summary>
    public List<Character> Bystanders(Roster roster) =>
        roster.Characters.Where(character => !Contains(character.Key)).ToList();

    /// <summary>
    /// Drop anyone who is no longer on the account. A deleted or transferred
    /// character would otherwise sit in the cohort forever, quietly keeping
    /// goals settled that nothing can any longer account for.
    /// </summary>
    public void Prune(Roster roster) => Members.RemoveWhere(key => roster.Get(key) is null);

    public bool Equals(Cohort? other) => other is not null && Members.SetEquals(other.Members);

    public override int GetHashCode() => Members.Count;
}
