using Microsoft.EntityFrameworkCore.Migrations;

#nullable disable

namespace Armory.Store.Migrations
{
    /// <inheritdoc />
    public partial class Initial : Migration
    {
        /// <inheritdoc />
        protected override void Up(MigrationBuilder migrationBuilder)
        {
            migrationBuilder.CreateTable(
                name: "achievement",
                columns: table => new
                {
                    id = table.Column<long>(type: "INTEGER", nullable: false),
                    name = table.Column<string>(type: "TEXT", nullable: false),
                    category = table.Column<string>(type: "TEXT", nullable: false),
                    points = table.Column<long>(type: "INTEGER", nullable: false),
                    description = table.Column<string>(type: "TEXT", nullable: false),
                    unrepeatable = table.Column<long>(type: "INTEGER", nullable: false)
                },
                constraints: table =>
                {
                    table.PrimaryKey("PK_achievement", x => x.id);
                });

            migrationBuilder.CreateTable(
                name: "attribution",
                columns: table => new
                {
                    achievement_id = table.Column<long>(type: "INTEGER", nullable: false),
                    realm_slug = table.Column<string>(type: "TEXT", nullable: false),
                    name = table.Column<string>(type: "TEXT", nullable: false)
                },
                constraints: table =>
                {
                    table.PrimaryKey("PK_attribution", x => x.achievement_id);
                });

            migrationBuilder.CreateTable(
                name: "change",
                columns: table => new
                {
                    seq = table.Column<long>(type: "INTEGER", nullable: false)
                        .Annotation("Sqlite:Autoincrement", true),
                    scope = table.Column<string>(type: "TEXT", nullable: false),
                    key = table.Column<string>(type: "TEXT", nullable: false),
                    gone = table.Column<long>(type: "INTEGER", nullable: false, defaultValue: 0L),
                    at = table.Column<string>(type: "TEXT", nullable: false),
                    machine = table.Column<string>(type: "TEXT", nullable: false, defaultValue: "")
                },
                constraints: table =>
                {
                    table.PrimaryKey("PK_change", x => x.seq);
                });

            migrationBuilder.CreateTable(
                name: "character",
                columns: table => new
                {
                    realm_slug = table.Column<string>(type: "TEXT", nullable: false),
                    name = table.Column<string>(type: "TEXT", nullable: false),
                    character_id = table.Column<long>(type: "INTEGER", nullable: false),
                    realm_id = table.Column<long>(type: "INTEGER", nullable: false),
                    display_name = table.Column<string>(type: "TEXT", nullable: false),
                    realm_name = table.Column<string>(type: "TEXT", nullable: false),
                    level = table.Column<long>(type: "INTEGER", nullable: false),
                    @class = table.Column<string>(name: "class", type: "TEXT", nullable: false),
                    race = table.Column<string>(type: "TEXT", nullable: false),
                    faction = table.Column<string>(type: "TEXT", nullable: false),
                    wow_account_id = table.Column<long>(type: "INTEGER", nullable: false)
                },
                constraints: table =>
                {
                    table.PrimaryKey("PK_character", x => new { x.realm_slug, x.name });
                });

            migrationBuilder.CreateTable(
                name: "collectible",
                columns: table => new
                {
                    kind = table.Column<string>(type: "TEXT", nullable: false),
                    id = table.Column<long>(type: "INTEGER", nullable: false),
                    json = table.Column<string>(type: "TEXT", nullable: false),
                    owned = table.Column<long>(type: "INTEGER", nullable: false, defaultValue: 0L)
                },
                constraints: table =>
                {
                    table.PrimaryKey("PK_collectible", x => new { x.kind, x.id });
                });

            migrationBuilder.CreateTable(
                name: "criterion",
                columns: table => new
                {
                    criterion_id = table.Column<long>(type: "INTEGER", nullable: false),
                    kind = table.Column<string>(type: "TEXT", nullable: false)
                },
                constraints: table =>
                {
                    table.PrimaryKey("PK_criterion", x => x.criterion_id);
                });

            migrationBuilder.CreateTable(
                name: "currency",
                columns: table => new
                {
                    realm_slug = table.Column<string>(type: "TEXT", nullable: false),
                    name = table.Column<string>(type: "TEXT", nullable: false),
                    currency_id = table.Column<long>(type: "INTEGER", nullable: false),
                    amount = table.Column<long>(type: "INTEGER", nullable: false)
                },
                constraints: table =>
                {
                    table.PrimaryKey("PK_currency", x => new { x.realm_slug, x.name, x.currency_id });
                });

            migrationBuilder.CreateTable(
                name: "detail",
                columns: table => new
                {
                    realm_slug = table.Column<string>(type: "TEXT", nullable: false),
                    name = table.Column<string>(type: "TEXT", nullable: false),
                    json = table.Column<string>(type: "TEXT", nullable: false),
                    fetched_at = table.Column<string>(type: "TEXT", nullable: false)
                },
                constraints: table =>
                {
                    table.PrimaryKey("PK_detail", x => new { x.realm_slug, x.name });
                });

            migrationBuilder.CreateTable(
                name: "earned_currency",
                columns: table => new
                {
                    realm_slug = table.Column<string>(type: "TEXT", nullable: false),
                    name = table.Column<string>(type: "TEXT", nullable: false),
                    currency_id = table.Column<long>(type: "INTEGER", nullable: false),
                    gained = table.Column<long>(type: "INTEGER", nullable: false),
                    earned = table.Column<long>(type: "INTEGER", nullable: false),
                    tracks_earned = table.Column<long>(type: "INTEGER", nullable: false),
                    account_wide = table.Column<long>(type: "INTEGER", nullable: false),
                    transferable = table.Column<long>(type: "INTEGER", nullable: false)
                },
                constraints: table =>
                {
                    table.PrimaryKey("PK_earned_currency", x => new { x.realm_slug, x.name, x.currency_id });
                });

            migrationBuilder.CreateTable(
                name: "earned_reputation",
                columns: table => new
                {
                    realm_slug = table.Column<string>(type: "TEXT", nullable: false),
                    name = table.Column<string>(type: "TEXT", nullable: false),
                    faction_id = table.Column<long>(type: "INTEGER", nullable: false),
                    points = table.Column<long>(type: "INTEGER", nullable: false),
                    renown = table.Column<long>(type: "INTEGER", nullable: false),
                    renown_seen = table.Column<long>(type: "INTEGER", nullable: false),
                    account_wide = table.Column<long>(type: "INTEGER", nullable: false)
                },
                constraints: table =>
                {
                    table.PrimaryKey("PK_earned_reputation", x => new { x.realm_slug, x.name, x.faction_id });
                });

            migrationBuilder.CreateTable(
                name: "encounter",
                columns: table => new
                {
                    id = table.Column<long>(type: "INTEGER", nullable: false),
                    name = table.Column<string>(type: "TEXT", nullable: false),
                    description = table.Column<string>(type: "TEXT", nullable: false),
                    loot = table.Column<string>(type: "TEXT", nullable: false)
                },
                constraints: table =>
                {
                    table.PrimaryKey("PK_encounter", x => x.id);
                });

            migrationBuilder.CreateTable(
                name: "enrolment",
                columns: table => new
                {
                    realm_slug = table.Column<string>(type: "TEXT", nullable: false),
                    name = table.Column<string>(type: "TEXT", nullable: false)
                },
                constraints: table =>
                {
                    table.PrimaryKey("PK_enrolment", x => new { x.realm_slug, x.name });
                });

            migrationBuilder.CreateTable(
                name: "entry",
                columns: table => new
                {
                    realm_slug = table.Column<string>(type: "TEXT", nullable: false),
                    name = table.Column<string>(type: "TEXT", nullable: false),
                    started_at = table.Column<string>(type: "TEXT", nullable: false),
                    title = table.Column<string>(type: "TEXT", nullable: false),
                    body = table.Column<string>(type: "TEXT", nullable: false),
                    model = table.Column<string>(type: "TEXT", nullable: false),
                    written_at = table.Column<string>(type: "TEXT", nullable: false)
                },
                constraints: table =>
                {
                    table.PrimaryKey("PK_entry", x => new { x.realm_slug, x.name, x.started_at });
                });

            migrationBuilder.CreateTable(
                name: "forgotten",
                columns: table => new
                {
                    realm_slug = table.Column<string>(type: "TEXT", nullable: false),
                    name = table.Column<string>(type: "TEXT", nullable: false),
                    started_at = table.Column<string>(type: "TEXT", nullable: false),
                    at = table.Column<string>(type: "TEXT", nullable: false)
                },
                constraints: table =>
                {
                    table.PrimaryKey("PK_forgotten", x => new { x.realm_slug, x.name, x.started_at });
                });

            migrationBuilder.CreateTable(
                name: "goal",
                columns: table => new
                {
                    run_id = table.Column<long>(type: "INTEGER", nullable: false),
                    achievement_id = table.Column<long>(type: "INTEGER", nullable: false),
                    standing = table.Column<string>(type: "TEXT", nullable: false),
                    bucket = table.Column<string>(type: "TEXT", nullable: false),
                    attestation = table.Column<string>(type: "TEXT", nullable: true)
                },
                constraints: table =>
                {
                    table.PrimaryKey("PK_goal", x => new { x.run_id, x.achievement_id });
                });

            migrationBuilder.CreateTable(
                name: "instance",
                columns: table => new
                {
                    id = table.Column<long>(type: "INTEGER", nullable: false),
                    name = table.Column<string>(type: "TEXT", nullable: false),
                    map = table.Column<long>(type: "INTEGER", nullable: true),
                    description = table.Column<string>(type: "TEXT", nullable: false),
                    expansion = table.Column<string>(type: "TEXT", nullable: false, defaultValue: ""),
                    encounters = table.Column<string>(type: "TEXT", nullable: false)
                },
                constraints: table =>
                {
                    table.PrimaryKey("PK_instance", x => x.id);
                });

            migrationBuilder.CreateTable(
                name: "item",
                columns: table => new
                {
                    item_id = table.Column<long>(type: "INTEGER", nullable: false),
                    name = table.Column<string>(type: "TEXT", nullable: false),
                    sellable = table.Column<long>(type: "INTEGER", nullable: false, defaultValue: 1L),
                    quality = table.Column<string>(type: "TEXT", nullable: false, defaultValue: "")
                },
                constraints: table =>
                {
                    table.PrimaryKey("PK_item", x => x.item_id);
                });

            migrationBuilder.CreateTable(
                name: "pet_held",
                columns: table => new
                {
                    species_id = table.Column<long>(type: "INTEGER", nullable: false),
                    count = table.Column<long>(type: "INTEGER", nullable: false)
                },
                constraints: table =>
                {
                    table.PrimaryKey("PK_pet_held", x => x.species_id);
                });

            migrationBuilder.CreateTable(
                name: "price",
                columns: table => new
                {
                    realm = table.Column<long>(type: "INTEGER", nullable: false),
                    item_id = table.Column<long>(type: "INTEGER", nullable: false),
                    variant = table.Column<string>(type: "TEXT", nullable: false),
                    seen_at = table.Column<string>(type: "TEXT", nullable: false),
                    unit_price = table.Column<long>(type: "INTEGER", nullable: false),
                    quantity = table.Column<long>(type: "INTEGER", nullable: false),
                    listings = table.Column<long>(type: "INTEGER", nullable: false, defaultValue: 0L),
                    tenth = table.Column<long>(type: "INTEGER", nullable: false, defaultValue: 0L),
                    median = table.Column<long>(type: "INTEGER", nullable: false, defaultValue: 0L)
                },
                constraints: table =>
                {
                    table.PrimaryKey("PK_price", x => new { x.realm, x.item_id, x.variant, x.seen_at });
                });

            migrationBuilder.CreateTable(
                name: "recipe",
                columns: table => new
                {
                    realm_slug = table.Column<string>(type: "TEXT", nullable: false),
                    name = table.Column<string>(type: "TEXT", nullable: false),
                    recipe_id = table.Column<long>(type: "INTEGER", nullable: false),
                    recipe = table.Column<string>(type: "TEXT", nullable: false),
                    output_id = table.Column<long>(type: "INTEGER", nullable: false),
                    makes = table.Column<long>(type: "INTEGER", nullable: false)
                },
                constraints: table =>
                {
                    table.PrimaryKey("PK_recipe", x => new { x.realm_slug, x.name, x.recipe_id });
                });

            migrationBuilder.CreateTable(
                name: "recipe_reagent",
                columns: table => new
                {
                    realm_slug = table.Column<string>(type: "TEXT", nullable: false),
                    name = table.Column<string>(type: "TEXT", nullable: false),
                    recipe_id = table.Column<long>(type: "INTEGER", nullable: false),
                    slot = table.Column<long>(type: "INTEGER", nullable: false),
                    quantity = table.Column<long>(type: "INTEGER", nullable: false),
                    tiers = table.Column<string>(type: "TEXT", nullable: false)
                },
                constraints: table =>
                {
                    table.PrimaryKey("PK_recipe_reagent", x => new { x.realm_slug, x.name, x.recipe_id, x.slot });
                });

            migrationBuilder.CreateTable(
                name: "response",
                columns: table => new
                {
                    url = table.Column<string>(type: "TEXT", nullable: false),
                    body = table.Column<byte[]>(type: "BLOB", nullable: false),
                    last_modified = table.Column<string>(type: "TEXT", nullable: true),
                    fetched_at = table.Column<string>(type: "TEXT", nullable: false)
                },
                constraints: table =>
                {
                    table.PrimaryKey("PK_response", x => x.url);
                });

            migrationBuilder.CreateTable(
                name: "run",
                columns: table => new
                {
                    id = table.Column<long>(type: "INTEGER", nullable: false)
                        .Annotation("Sqlite:Autoincrement", true),
                    name = table.Column<string>(type: "TEXT", nullable: false),
                    baseline = table.Column<string>(type: "TEXT", nullable: false),
                    cohort = table.Column<string>(type: "TEXT", nullable: false),
                    is_current = table.Column<long>(type: "INTEGER", nullable: false, defaultValue: 0L),
                    key = table.Column<string>(type: "TEXT", nullable: false, defaultValue: "")
                },
                constraints: table =>
                {
                    table.PrimaryKey("PK_run", x => x.id);
                });

            migrationBuilder.CreateTable(
                name: "session",
                columns: table => new
                {
                    realm_slug = table.Column<string>(type: "TEXT", nullable: false),
                    name = table.Column<string>(type: "TEXT", nullable: false),
                    started_at = table.Column<string>(type: "TEXT", nullable: false),
                    ended_at = table.Column<string>(type: "TEXT", nullable: false),
                    json = table.Column<string>(type: "TEXT", nullable: false)
                },
                constraints: table =>
                {
                    table.PrimaryKey("PK_session", x => new { x.realm_slug, x.name, x.started_at });
                });

            migrationBuilder.CreateTable(
                name: "snapshot",
                columns: table => new
                {
                    realm = table.Column<long>(type: "INTEGER", nullable: false),
                    item_id = table.Column<long>(type: "INTEGER", nullable: false),
                    variant = table.Column<string>(type: "TEXT", nullable: false),
                    cheapest = table.Column<long>(type: "INTEGER", nullable: false),
                    quantity = table.Column<long>(type: "INTEGER", nullable: false),
                    listings = table.Column<long>(type: "INTEGER", nullable: false),
                    tenth = table.Column<long>(type: "INTEGER", nullable: false),
                    median = table.Column<long>(type: "INTEGER", nullable: false),
                    seen_at = table.Column<string>(type: "TEXT", nullable: false)
                },
                constraints: table =>
                {
                    table.PrimaryKey("PK_snapshot", x => new { x.realm, x.item_id, x.variant });
                });

            migrationBuilder.CreateTable(
                name: "sync_state",
                columns: table => new
                {
                    name = table.Column<string>(type: "TEXT", nullable: false),
                    value = table.Column<string>(type: "TEXT", nullable: false)
                },
                constraints: table =>
                {
                    table.PrimaryKey("PK_sync_state", x => x.name);
                });

            migrationBuilder.CreateTable(
                name: "tally",
                columns: table => new
                {
                    realm_slug = table.Column<string>(type: "TEXT", nullable: false),
                    name = table.Column<string>(type: "TEXT", nullable: false),
                    kind = table.Column<string>(type: "TEXT", nullable: false),
                    key = table.Column<string>(type: "TEXT", nullable: false),
                    count = table.Column<long>(type: "INTEGER", nullable: false),
                    label = table.Column<string>(type: "TEXT", nullable: false)
                },
                constraints: table =>
                {
                    table.PrimaryKey("PK_tally", x => new { x.realm_slug, x.name, x.kind, x.key });
                });

            migrationBuilder.CreateTable(
                name: "warband_item",
                columns: table => new
                {
                    item_id = table.Column<long>(type: "INTEGER", nullable: false),
                    count = table.Column<long>(type: "INTEGER", nullable: false)
                },
                constraints: table =>
                {
                    table.PrimaryKey("PK_warband_item", x => x.item_id);
                });

            migrationBuilder.CreateTable(
                name: "watched",
                columns: table => new
                {
                    item_id = table.Column<long>(type: "INTEGER", nullable: false),
                    name = table.Column<string>(type: "TEXT", nullable: false, defaultValue: "")
                },
                constraints: table =>
                {
                    table.PrimaryKey("PK_watched", x => x.item_id);
                });

            migrationBuilder.CreateTable(
                name: "watched_realm",
                columns: table => new
                {
                    realm_id = table.Column<long>(type: "INTEGER", nullable: false),
                    name = table.Column<string>(type: "TEXT", nullable: false, defaultValue: "")
                },
                constraints: table =>
                {
                    table.PrimaryKey("PK_watched_realm", x => x.realm_id);
                });

            migrationBuilder.CreateIndex(
                name: "change_machine",
                table: "change",
                columns: new[] { "machine", "seq" });

            migrationBuilder.CreateIndex(
                name: "change_scope_key",
                table: "change",
                columns: new[] { "scope", "key" },
                unique: true);

            migrationBuilder.CreateIndex(
                name: "price_item",
                table: "price",
                columns: new[] { "item_id", "realm", "seen_at" });

            migrationBuilder.CreateIndex(
                name: "price_seen_at",
                table: "price",
                column: "seen_at");

            migrationBuilder.CreateIndex(
                name: "response_fetched_at",
                table: "response",
                column: "fetched_at");

            migrationBuilder.CreateIndex(
                name: "run_key",
                table: "run",
                column: "key",
                unique: true);

            migrationBuilder.CreateIndex(
                name: "session_started_at",
                table: "session",
                column: "started_at");
        }

        /// <inheritdoc />
        protected override void Down(MigrationBuilder migrationBuilder)
        {
            migrationBuilder.DropTable(
                name: "achievement");

            migrationBuilder.DropTable(
                name: "attribution");

            migrationBuilder.DropTable(
                name: "change");

            migrationBuilder.DropTable(
                name: "character");

            migrationBuilder.DropTable(
                name: "collectible");

            migrationBuilder.DropTable(
                name: "criterion");

            migrationBuilder.DropTable(
                name: "currency");

            migrationBuilder.DropTable(
                name: "detail");

            migrationBuilder.DropTable(
                name: "earned_currency");

            migrationBuilder.DropTable(
                name: "earned_reputation");

            migrationBuilder.DropTable(
                name: "encounter");

            migrationBuilder.DropTable(
                name: "enrolment");

            migrationBuilder.DropTable(
                name: "entry");

            migrationBuilder.DropTable(
                name: "forgotten");

            migrationBuilder.DropTable(
                name: "goal");

            migrationBuilder.DropTable(
                name: "instance");

            migrationBuilder.DropTable(
                name: "item");

            migrationBuilder.DropTable(
                name: "pet_held");

            migrationBuilder.DropTable(
                name: "price");

            migrationBuilder.DropTable(
                name: "recipe");

            migrationBuilder.DropTable(
                name: "recipe_reagent");

            migrationBuilder.DropTable(
                name: "response");

            migrationBuilder.DropTable(
                name: "run");

            migrationBuilder.DropTable(
                name: "session");

            migrationBuilder.DropTable(
                name: "snapshot");

            migrationBuilder.DropTable(
                name: "sync_state");

            migrationBuilder.DropTable(
                name: "tally");

            migrationBuilder.DropTable(
                name: "warband_item");

            migrationBuilder.DropTable(
                name: "watched");

            migrationBuilder.DropTable(
                name: "watched_realm");
        }
    }
}
