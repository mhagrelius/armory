using System.Text;
using Microsoft.EntityFrameworkCore;
using Microsoft.EntityFrameworkCore.Design;

namespace Armory.Store;

/// <summary>
/// The schema. Every table and column is named as <c>core/src/store.rs</c>
/// names it, because <see cref="Sharing.Tables"/> puts rows on the wire by
/// position over those names and the Rust server holds the same tables.
/// </summary>
/// <remarks>
/// Why each table exists, briefly. <c>detail</c> is the expensive half of a
/// character as JSON: read whole, never queried across, grows a field every
/// time Blizzard adds an endpoint. <c>attribution</c> is who first earned
/// each account-wide achievement, from the addon and nowhere else; it decides
/// poisoning. <c>earned_reputation</c>, <c>earned_currency</c> and
/// <c>tally</c> are cumulative records of work done and merge by MAX, never
/// replaced. <c>recipe</c> merges because the addon reads one profession at a
/// time. <c>instance</c> and <c>encounter</c> are Blizzard's Adventure Guide
/// prose, fetched and kept like the achievement catalogue. <c>run</c> has a
/// local autoincrement id and a stable <c>key</c> derived from its baseline,
/// because two machines pick different ids for the same run. <c>price</c> is
/// history, opt-in and under the thirty-day term; <c>snapshot</c> is now,
/// replaced whole each hour, free. <c>session</c> and <c>entry</c> are the
/// journal and are never purged: they came off the addon, not the API.
/// <c>forgotten</c> travels because forgetting is a decision. <c>response</c>
/// is the API cache with the Last-Modified that makes the next request
/// conditional.
/// </remarks>
public sealed class ArmoryContext : DbContext
{
    public ArmoryContext(DbContextOptions<ArmoryContext> options)
        : base(options)
    {
    }

    public DbSet<CharacterRow> Characters => Set<CharacterRow>();
    public DbSet<EnrolmentRow> Enrolments => Set<EnrolmentRow>();
    public DbSet<DetailRow> Details => Set<DetailRow>();
    public DbSet<AttributionRow> Attributions => Set<AttributionRow>();
    public DbSet<CurrencyRow> Currencies => Set<CurrencyRow>();
    public DbSet<EarnedReputationRow> EarnedReputations => Set<EarnedReputationRow>();
    public DbSet<EarnedCurrencyRow> EarnedCurrencies => Set<EarnedCurrencyRow>();
    public DbSet<TallyRow> Tallies => Set<TallyRow>();
    public DbSet<RecipeRow> Recipes => Set<RecipeRow>();
    public DbSet<RecipeReagentRow> RecipeReagents => Set<RecipeReagentRow>();
    public DbSet<InstanceRow> Instances => Set<InstanceRow>();
    public DbSet<EncounterRow> Encounters => Set<EncounterRow>();
    public DbSet<CriterionRow> Criteria => Set<CriterionRow>();
    public DbSet<WarbandItemRow> WarbandItems => Set<WarbandItemRow>();
    public DbSet<PetHeldRow> PetsHeld => Set<PetHeldRow>();
    public DbSet<GoalRow> Goals => Set<GoalRow>();
    public DbSet<RunRow> Runs => Set<RunRow>();
    public DbSet<CollectibleRow> Collectibles => Set<CollectibleRow>();
    public DbSet<AchievementRow> Achievements => Set<AchievementRow>();
    public DbSet<PriceRow> Prices => Set<PriceRow>();
    public DbSet<SnapshotRow> Snapshots => Set<SnapshotRow>();
    public DbSet<ItemRow> Items => Set<ItemRow>();
    public DbSet<WatchedRow> Watched => Set<WatchedRow>();
    public DbSet<WatchedRealmRow> WatchedRealms => Set<WatchedRealmRow>();
    public DbSet<SessionRow> Sessions => Set<SessionRow>();
    public DbSet<EntryRow> Entries => Set<EntryRow>();
    public DbSet<ResponseRow> Responses => Set<ResponseRow>();
    public DbSet<ForgottenRow> Forgotten => Set<ForgottenRow>();
    public DbSet<ChangeRow> Changes => Set<ChangeRow>();
    public DbSet<SyncStateRow> SyncState => Set<SyncStateRow>();

    protected override void OnModelCreating(ModelBuilder modelBuilder)
    {
        modelBuilder.Entity<CharacterRow>(e =>
        {
            e.ToTable("character");
            e.HasKey(r => new { r.RealmSlug, r.Name });
        });
        modelBuilder.Entity<EnrolmentRow>(e =>
        {
            e.ToTable("enrolment");
            e.HasKey(r => new { r.RealmSlug, r.Name });
        });
        modelBuilder.Entity<DetailRow>(e =>
        {
            e.ToTable("detail");
            e.HasKey(r => new { r.RealmSlug, r.Name });
        });
        modelBuilder.Entity<AttributionRow>(e =>
        {
            e.ToTable("attribution");
            e.HasKey(r => r.AchievementId);
            e.Property(r => r.AchievementId).ValueGeneratedNever();
        });
        modelBuilder.Entity<CurrencyRow>(e =>
        {
            e.ToTable("currency");
            e.HasKey(r => new { r.RealmSlug, r.Name, r.CurrencyId });
        });
        modelBuilder.Entity<EarnedReputationRow>(e =>
        {
            e.ToTable("earned_reputation");
            e.HasKey(r => new { r.RealmSlug, r.Name, r.FactionId });
        });
        modelBuilder.Entity<EarnedCurrencyRow>(e =>
        {
            e.ToTable("earned_currency");
            e.HasKey(r => new { r.RealmSlug, r.Name, r.CurrencyId });
        });
        modelBuilder.Entity<TallyRow>(e =>
        {
            e.ToTable("tally");
            e.HasKey(r => new { r.RealmSlug, r.Name, r.Kind, r.Key });
        });
        modelBuilder.Entity<RecipeRow>(e =>
        {
            e.ToTable("recipe");
            e.HasKey(r => new { r.RealmSlug, r.Name, r.RecipeId });
        });
        modelBuilder.Entity<RecipeReagentRow>(e =>
        {
            e.ToTable("recipe_reagent");
            e.HasKey(r => new { r.RealmSlug, r.Name, r.RecipeId, r.Slot });
        });
        modelBuilder.Entity<InstanceRow>(e =>
        {
            e.ToTable("instance");
            e.HasKey(r => r.Id);
            e.Property(r => r.Id).ValueGeneratedNever();
            e.Property(r => r.Expansion).HasDefaultValue("");
        });
        modelBuilder.Entity<EncounterRow>(e =>
        {
            e.ToTable("encounter");
            e.HasKey(r => r.Id);
            e.Property(r => r.Id).ValueGeneratedNever();
        });
        modelBuilder.Entity<CriterionRow>(e =>
        {
            e.ToTable("criterion");
            e.HasKey(r => r.CriterionId);
            e.Property(r => r.CriterionId).ValueGeneratedNever();
        });
        modelBuilder.Entity<WarbandItemRow>(e =>
        {
            e.ToTable("warband_item");
            e.HasKey(r => r.ItemId);
            e.Property(r => r.ItemId).ValueGeneratedNever();
        });
        modelBuilder.Entity<PetHeldRow>(e =>
        {
            e.ToTable("pet_held");
            e.HasKey(r => r.SpeciesId);
            e.Property(r => r.SpeciesId).ValueGeneratedNever();
        });
        modelBuilder.Entity<GoalRow>(e =>
        {
            e.ToTable("goal");
            e.HasKey(r => new { r.RunId, r.AchievementId });
        });
        modelBuilder.Entity<RunRow>(e =>
        {
            e.ToTable("run");
            e.HasKey(r => r.Id);
            e.Property(r => r.Id).ValueGeneratedOnAdd().HasAnnotation("Sqlite:Autoincrement", true);
            e.Property(r => r.IsCurrent).HasDefaultValue(0L);
            e.Property(r => r.Key).HasDefaultValue("");
            e.HasIndex(r => r.Key).IsUnique().HasDatabaseName("run_key");
        });
        modelBuilder.Entity<CollectibleRow>(e =>
        {
            e.ToTable("collectible");
            e.HasKey(r => new { r.Kind, r.Id });
            e.Property(r => r.Owned).HasDefaultValue(0L);
        });
        modelBuilder.Entity<AchievementRow>(e =>
        {
            e.ToTable("achievement");
            e.HasKey(r => r.Id);
            e.Property(r => r.Id).ValueGeneratedNever();
        });
        modelBuilder.Entity<PriceRow>(e =>
        {
            e.ToTable("price");
            e.HasKey(r => new { r.Realm, r.ItemId, r.Variant, r.SeenAt });
            e.Property(r => r.Listings).HasDefaultValue(0L);
            e.Property(r => r.Tenth).HasDefaultValue(0L);
            e.Property(r => r.Median).HasDefaultValue(0L);
            e.HasIndex(r => new { r.ItemId, r.Realm, r.SeenAt }).HasDatabaseName("price_item");
            e.HasIndex(r => r.SeenAt).HasDatabaseName("price_seen_at");
        });
        modelBuilder.Entity<SnapshotRow>(e =>
        {
            e.ToTable("snapshot");
            e.HasKey(r => new { r.Realm, r.ItemId, r.Variant });
        });
        modelBuilder.Entity<ItemRow>(e =>
        {
            e.ToTable("item");
            e.HasKey(r => r.ItemId);
            e.Property(r => r.ItemId).ValueGeneratedNever();
            e.Property(r => r.Sellable).HasDefaultValue(1L);
            e.Property(r => r.Quality).HasDefaultValue("");
        });
        modelBuilder.Entity<WatchedRow>(e =>
        {
            e.ToTable("watched");
            e.HasKey(r => r.ItemId);
            e.Property(r => r.ItemId).ValueGeneratedNever();
            e.Property(r => r.Name).HasDefaultValue("");
        });
        modelBuilder.Entity<WatchedRealmRow>(e =>
        {
            e.ToTable("watched_realm");
            e.HasKey(r => r.RealmId);
            e.Property(r => r.RealmId).ValueGeneratedNever();
            e.Property(r => r.Name).HasDefaultValue("");
        });
        modelBuilder.Entity<SessionRow>(e =>
        {
            e.ToTable("session");
            e.HasKey(r => new { r.RealmSlug, r.Name, r.StartedAt });
            e.HasIndex(r => r.StartedAt).HasDatabaseName("session_started_at");
        });
        modelBuilder.Entity<EntryRow>(e =>
        {
            e.ToTable("entry");
            e.HasKey(r => new { r.RealmSlug, r.Name, r.StartedAt });
        });
        modelBuilder.Entity<ResponseRow>(e =>
        {
            e.ToTable("response");
            e.HasKey(r => r.Url);
            e.HasIndex(r => r.FetchedAt).HasDatabaseName("response_fetched_at");
        });
        modelBuilder.Entity<ForgottenRow>(e =>
        {
            e.ToTable("forgotten");
            e.HasKey(r => new { r.RealmSlug, r.Name, r.StartedAt });
        });
        modelBuilder.Entity<ChangeRow>(e =>
        {
            e.ToTable("change");
            e.HasKey(r => r.Seq);
            e.Property(r => r.Seq).ValueGeneratedOnAdd().HasAnnotation("Sqlite:Autoincrement", true);
            e.Property(r => r.Gone).HasDefaultValue(0L);
            e.Property(r => r.Machine).HasDefaultValue("");
            e.HasIndex(r => new { r.Scope, r.Key }).IsUnique().HasDatabaseName("change_scope_key");
            e.HasIndex(r => new { r.Machine, r.Seq }).HasDatabaseName("change_machine");
        });
        modelBuilder.Entity<SyncStateRow>(e =>
        {
            e.ToTable("sync_state");
            e.HasKey(r => r.Name);
        });

        // Every column is the property's name in snake_case, which is the
        // Rust store's spelling. Set once here rather than on every property,
        // so a column added later cannot be spelled two ways.
        foreach (var entity in modelBuilder.Model.GetEntityTypes())
        {
            foreach (var property in entity.GetProperties())
            {
                property.SetColumnName(SnakeCase(property.Name));
            }
        }
    }

    internal static string SnakeCase(string name)
    {
        var builder = new StringBuilder(name.Length + 4);
        for (var i = 0; i < name.Length; i++)
        {
            var c = name[i];
            if (char.IsUpper(c) && i > 0)
            {
                builder.Append('_');
            }
            builder.Append(char.ToLowerInvariant(c));
        }
        return builder.ToString();
    }
}

/// <summary>What <c>dotnet ef migrations add</c> builds a context with.</summary>
public sealed class DesignTimeContextFactory : IDesignTimeDbContextFactory<ArmoryContext>
{
    public ArmoryContext CreateDbContext(string[] args)
    {
        var options = new DbContextOptionsBuilder<ArmoryContext>()
            .UseSqlite("Data Source=design-time.db")
            .Options;
        return new ArmoryContext(options);
    }
}
