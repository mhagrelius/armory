using System.Globalization;
using System.Text.Json;
using Armory.Addon;
using Armory.Market;
using Armory.Provenance;
using Armory.Roster;
using Armory.Run;
using Armory.Tally;
using Microsoft.EntityFrameworkCore;

namespace Armory.Store;

/// <summary>What the addon collected: attribution, criteria, currencies, provenance, tallies, recipes, the bank.</summary>
public sealed partial class Store
{
    /// <summary>
    /// Write everything the collector addon reported.
    /// </summary>
    /// <remarks>
    /// Attribution, currencies, criteria and the bank are reconciled: the
    /// addon rewrites its file in full on every logout, and a stale
    /// attribution un-poisons a goal that should be poisoned. Earned
    /// reputation, earned currency and the tallies are merged by taking the
    /// larger count: they are a record of what somebody did over months, and
    /// a reinstalled addon starts from zero. Recipes merge because the addon
    /// reads one profession window at a time, and a recipe's reagent slots
    /// are reconciled one recipe at a time so a changed recipe does not keep
    /// both sets.
    /// </remarks>
    public Result<Unit, StoreError> SaveCollected(Collected collected) => Work(() =>
    {
        Reconcile(
            Context.Attributions,
            collected.EarnedBy.Select(pair => new AttributionRow { AchievementId = pair.Key, RealmSlug = pair.Value.RealmSlug, Name = pair.Value.Name }).ToList(),
            row => row.AchievementId,
            (held, fresh) =>
            {
                held.RealmSlug = fresh.RealmSlug;
                held.Name = fresh.Name;
            });

        Reconcile(
            Context.Currencies,
            collected.Currencies
                .SelectMany(pair => pair.Value.Select(amount => new CurrencyRow { RealmSlug = pair.Key.RealmSlug, Name = pair.Key.Name, CurrencyId = amount.Key, Amount = amount.Value }))
                .ToList(),
            row => (row.RealmSlug, row.Name, row.CurrencyId),
            (held, fresh) => held.Amount = fresh.Amount);

        foreach (var (key, earned) in collected.Earned)
        {
            foreach (var (faction, with) in earned.Reputation)
            {
                var held = Context.EarnedReputations.Find(key.RealmSlug, key.Name, faction);
                if (held is null)
                {
                    Context.EarnedReputations.Add(new EarnedReputationRow
                    {
                        RealmSlug = key.RealmSlug,
                        Name = key.Name,
                        FactionId = faction,
                        Points = with.Points,
                        Renown = with.Renown,
                        RenownSeen = with.RenownSeen,
                        AccountWide = with.AccountWide ? 1 : 0,
                    });
                }
                else
                {
                    held.Points = Math.Max(held.Points, with.Points);
                    held.Renown = Math.Max(held.Renown, with.Renown);
                    held.RenownSeen = Math.Max(held.RenownSeen, with.RenownSeen);
                    held.AccountWide = with.AccountWide ? 1 : 0;
                }
            }
            foreach (var (currency, gained) in earned.Currency)
            {
                var held = Context.EarnedCurrencies.Find(key.RealmSlug, key.Name, currency);
                if (held is null)
                {
                    Context.EarnedCurrencies.Add(new EarnedCurrencyRow
                    {
                        RealmSlug = key.RealmSlug,
                        Name = key.Name,
                        CurrencyId = currency,
                        Gained = gained.Gained,
                        Earned = gained.Earned,
                        TracksEarned = gained.TracksEarned ? 1 : 0,
                        AccountWide = gained.AccountWide ? 1 : 0,
                        Transferable = gained.Transferable ? 1 : 0,
                    });
                }
                else
                {
                    held.Gained = Math.Max(held.Gained, gained.Gained);
                    held.Earned = Math.Max(held.Earned, gained.Earned);
                    held.TracksEarned = gained.TracksEarned ? 1 : 0;
                    held.AccountWide = gained.AccountWide ? 1 : 0;
                    held.Transferable = gained.Transferable ? 1 : 0;
                }
            }
        }

        foreach (var (character, counted) in collected.Tallies)
        {
            foreach (var tally in counted)
            {
                var held = Context.Tallies.Find(character.RealmSlug, character.Name, tally.Kind.Token(), tally.Key);
                if (held is null)
                {
                    Context.Tallies.Add(new TallyRow
                    {
                        RealmSlug = character.RealmSlug,
                        Name = character.Name,
                        Kind = tally.Kind.Token(),
                        Key = tally.Key,
                        Count = tally.Count,
                        Label = tally.Label,
                    });
                }
                else
                {
                    held.Count = Math.Max(held.Count, tally.Count);
                    held.Label = tally.Label;
                }
            }
        }

        foreach (var (character, book) in collected.Recipes)
        {
            foreach (var recipe in book)
            {
                var held = Context.Recipes.Find(character.RealmSlug, character.Name, recipe.Id);
                if (held is null)
                {
                    Context.Recipes.Add(new RecipeRow
                    {
                        RealmSlug = character.RealmSlug,
                        Name = character.Name,
                        RecipeId = recipe.Id,
                        Recipe = recipe.Name,
                        OutputId = recipe.Output,
                        Makes = recipe.Makes,
                    });
                }
                else
                {
                    held.Recipe = recipe.Name;
                    held.OutputId = recipe.Output;
                    held.Makes = recipe.Makes;
                }

                var slots = recipe.Reagents.Select((reagent, index) => new RecipeReagentRow
                {
                    RealmSlug = character.RealmSlug,
                    Name = character.Name,
                    RecipeId = recipe.Id,
                    Slot = index,
                    Quantity = reagent.Quantity,
                    Tiers = string.Join(",", reagent.Tiers.Select(tier => tier.ToString(CultureInfo.InvariantCulture))),
                }).ToList();
                var realm = character.RealmSlug;
                var name = character.Name;
                var recipeId = recipe.Id;
                Reconcile(
                    Context.RecipeReagents.Where(row => row.RealmSlug == realm && row.Name == name && row.RecipeId == recipeId),
                    slots,
                    row => row.Slot,
                    (existing, fresh) =>
                    {
                        existing.Quantity = fresh.Quantity;
                        existing.Tiers = fresh.Tiers;
                    });
            }
        }

        Reconcile(
            Context.Criteria,
            collected.Criteria.Select(pair => new CriterionRow { CriterionId = pair.Key, Kind = JsonSerializer.Serialize(pair.Value) }).ToList(),
            row => row.CriterionId,
            (held, fresh) => held.Kind = fresh.Kind);

        Reconcile(
            Context.WarbandItems,
            collected.WarbandBank.Select(pair => new WarbandItemRow { ItemId = pair.Key, Count = pair.Value }).ToList(),
            row => row.ItemId,
            (held, fresh) => held.Count = fresh.Count);

        // Replaced wholesale, like the bank: a species that has dropped out of
        // the file is one the journal no longer holds. Only when the file
        // carried counts at all, though: an older collector writes none, and
        // emptying the table on its say-so would silently un-spare everything.
        if (collected.PetsHeld.Count > 0)
        {
            Reconcile(
                Context.PetsHeld,
                collected.PetsHeld.Select(pair => new PetHeldRow { SpeciesId = pair.Key, Count = pair.Value }).ToList(),
                row => row.SpeciesId,
                (held, fresh) => held.Count = fresh.Count);
        }
    });

    /// <summary>Who earned each account-wide achievement.</summary>
    public Result<Dictionary<long, CharacterKey>, StoreError> Attributions() => Work(() =>
        Context.Attributions.AsNoTracking().AsEnumerable().ToDictionary(row => row.AchievementId, row => new CharacterKey(row.RealmSlug, row.Name)));

    /// <summary>What each criterion measures.</summary>
    public Result<Dictionary<long, CriterionKind>, StoreError> CriteriaKinds() => Work(() =>
    {
        var result = new Dictionary<long, CriterionKind>();
        foreach (var row in Context.Criteria.AsNoTracking())
        {
            try
            {
                result[row.CriterionId] = JsonSerializer.Deserialize<CriterionKind>(row.Kind);
            }
            catch (JsonException)
            {
                // A kind a newer build wrote; skipped for the reason the addon reader skips it.
            }
        }
        return result;
    });

    /// <summary>Currencies, per character.</summary>
    public Result<Dictionary<CharacterKey, Dictionary<long, long>>, StoreError> CurrenciesHeld() => Work(() =>
    {
        var result = new Dictionary<CharacterKey, Dictionary<long, long>>();
        foreach (var row in Context.Currencies.AsNoTracking())
        {
            var key = new CharacterKey(row.RealmSlug, row.Name);
            if (!result.TryGetValue(key, out var amounts))
            {
                amounts = new Dictionary<long, long>();
                result[key] = amounts;
            }
            amounts[row.CurrencyId] = row.Amount;
        }
        return result;
    });

    /// <summary>What each character has personally earned, reputation and currency both. The only source for it.</summary>
    public Result<Earnings, StoreError> ProvenanceHeld() => Work(() =>
    {
        var result = new Earnings();
        foreach (var row in Context.EarnedReputations.AsNoTracking())
        {
            var key = new CharacterKey(row.RealmSlug, row.Name);
            if (!result.TryGetValue(key, out var earned))
            {
                earned = new Earned();
                result[key] = earned;
            }
            earned.Reputation[row.FactionId] = new EarnedReputation
            {
                Points = row.Points,
                Renown = row.Renown,
                RenownSeen = row.RenownSeen,
                AccountWide = row.AccountWide != 0,
            };
        }
        foreach (var row in Context.EarnedCurrencies.AsNoTracking())
        {
            var key = new CharacterKey(row.RealmSlug, row.Name);
            if (!result.TryGetValue(key, out var earned))
            {
                earned = new Earned();
                result[key] = earned;
            }
            earned.Currency[row.CurrencyId] = new EarnedCurrency
            {
                Gained = row.Gained,
                Earned = row.Earned,
                TracksEarned = row.TracksEarned != 0,
                AccountWide = row.AccountWide != 0,
                Transferable = row.Transferable != 0,
            };
        }
        return result;
    });

    /// <summary>
    /// Every counter, per character, biggest first. Read back rather than
    /// taken from the dump: the write merges by taking the larger count, so
    /// the dump alone is the wrong number once an addon folder has been
    /// cleared.
    /// </summary>
    public Result<Tallies, StoreError> TalliesHeld() => Work(() =>
    {
        var result = new Tallies();
        foreach (var row in Context.Tallies.AsNoTracking().OrderByDescending(row => row.Count).ThenBy(row => row.Label))
        {
            // A row written by a newer Armory against an older one's database.
            if (CountingExtensions.FromToken(row.Kind) is not { } kind)
            {
                continue;
            }
            var key = new CharacterKey(row.RealmSlug, row.Name);
            if (!result.TryGetValue(key, out var counted))
            {
                counted = new List<Tally.Tally>();
                result[key] = counted;
            }
            counted.Add(new Tally.Tally { Kind = kind, Key = row.Key, Label = row.Label, Count = row.Count });
        }
        return result;
    });

    /// <summary>What every character can make. A recipe row whose slots did not survive is not a free recipe, it is an unreadable one.</summary>
    public Result<RecipeBooks, StoreError> RecipesHeld() => Work(() =>
    {
        var slots = new Dictionary<(string, string, long), List<Reagent>>();
        foreach (var row in Context.RecipeReagents.AsNoTracking().OrderBy(row => row.Slot))
        {
            var tiers = ParseTiers(row.Tiers);
            if (tiers.Count == 0)
            {
                continue;
            }
            var key = (row.RealmSlug, row.Name, row.RecipeId);
            if (!slots.TryGetValue(key, out var reagents))
            {
                reagents = new List<Reagent>();
                slots[key] = reagents;
            }
            reagents.Add(new Reagent { Quantity = row.Quantity, Tiers = tiers });
        }

        var result = new RecipeBooks();
        foreach (var row in Context.Recipes.AsNoTracking())
        {
            if (!slots.Remove((row.RealmSlug, row.Name, row.RecipeId), out var reagents))
            {
                continue;
            }
            var who = new CharacterKey(row.RealmSlug, row.Name);
            if (!result.TryGetValue(who, out var book))
            {
                book = new List<Recipe>();
                result[who] = book;
            }
            book.Add(new Recipe { Id = row.RecipeId, Name = row.Recipe, Output = row.OutputId, Makes = row.Makes, Reagents = reagents });
        }
        return result;
    });

    /// <summary>Every item any known recipe names, as a reagent or as its output. What the auction snapshots are filtered against.</summary>
    public Result<HashSet<long>, StoreError> RecipeItems() => Work(() =>
    {
        var result = Context.Recipes.AsNoTracking().Select(row => row.OutputId).ToHashSet();
        foreach (var tiers in Context.RecipeReagents.AsNoTracking().Select(row => row.Tiers))
        {
            result.UnionWith(ParseTiers(tiers));
        }
        return result;
    });

    /// <summary>The Warband bank.</summary>
    public Result<Dictionary<long, long>, StoreError> WarbandBank() => Work(() =>
        Context.WarbandItems.AsNoTracking().ToDictionary(row => row.ItemId, row => row.Count));

    /// <summary>How many of each pet species the journal holds.</summary>
    public Result<Dictionary<long, long>, StoreError> PetsHeldCounts() => Work(() =>
        Context.PetsHeld.AsNoTracking().ToDictionary(row => row.SpeciesId, row => row.Count));

    /// <summary>A joined list of ids back into numbers, skipping anything that is not one.</summary>
    private static List<long> ParseTiers(string tiers)
    {
        var result = new List<long>();
        foreach (var id in tiers.Split(','))
        {
            if (long.TryParse(id, NumberStyles.Integer, CultureInfo.InvariantCulture, out var tier))
            {
                result.Add(tier);
            }
        }
        return result;
    }
}
