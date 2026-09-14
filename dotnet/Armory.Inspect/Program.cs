// Read a real collector dump and say what is in it.
//
// The addon's calls are documented but the returns are not contractual, and
// several of them — `GetAchievementInfo`'s `earnedBy`, the journals' extra-info
// shapes, `Enum.BagIndex` for Warband tabs — are the sort of thing that
// silently returns nil against a live client. A parser that quietly produces an
// empty map looks identical to an account with nothing in it, so this prints
// the counts and lets a person see which it is.
//
//   dotnet run --project Armory.Inspect
//   dotnet run --project Armory.Inspect -- "C:\Program Files (x86)\World of Warcraft\_retail_"
//   dotnet run --project Armory.Inspect -- "C:\Program Files (x86)\World of Warcraft\_retail_" RAELTO
//
// The port of `examples/inspect.rs`: the same sections, in the same order.

using System.Text;
using Armory.Addon;
using Armory.Client.Shell;
using Armory.Collections;
using Armory.Roster;
using Armory.Run;

Console.OutputEncoding = Encoding.UTF8;

var wow = args.Length > 0
    ? args[0]
    : Armory.Settings.Settings.FindWow(Environment.GetFolderPath(Environment.SpecialFolder.UserProfile));
if (wow is null)
{
    Console.Error.WriteLine("no WoW install found — pass the path to _retail_");
    return 1;
}
Console.WriteLine($"install: {wow}");

// The Rust takes the first account folder. So does the app, until Settings
// names one; a second argument stands in for that setting here, because the
// launcher can leave a numeric folder that sorts ahead of the real login.
var account = args.Length > 1 ? args[1] : Files.Accounts(wow).FirstOrDefault();
if (account is null)
{
    Console.Error.WriteLine("no account folder under WTF/Account");
    return 1;
}
Console.WriteLine($"account: {account}\n");

// -- the account-wide file ----------------------------------------------

var path = AddonWatch.AccountFile(wow, account);
var read = AddonWatch.ReadAccount(path);
if (!read.IsOk)
{
    Console.Error.WriteLine($"could not read {path}: {read.Error}");
    return 1;
}
var collected = read.Value;

Console.WriteLine("ACCOUNT");
Console.WriteLine($"  written at        {(collected.WrittenAt is { } written ? $"Some({Stamps.Rfc3339(written)})" : "None")}");
Console.WriteLine($"  attributions      {collected.EarnedBy.Count}");
Console.WriteLine($"  completed         {collected.Completed.Count}");
Console.WriteLine($"  criteria trees    {collected.Tree.Count}");
Console.WriteLine($"  criteria mapped   {collected.Criteria.Count}");

var understood = collected.Criteria.Values.Count(kind => kind.IsObservable);
Console.WriteLine($"  of those, understood {understood} ({Percent(understood, collected.Criteria.Count):F0}%)");

Console.WriteLine($"  warband bank      {collected.WarbandBank.Count} items");
Console.WriteLine($"  currencies        {collected.Currencies.Count} characters");

foreach (var kind in new[] { Kind.Mount, Kind.Pet, Kind.Toy, Kind.Decor })
{
    var all = collected.Collectibles.Where(entry => entry.Kind == kind).ToList();
    var owned = all.Count(entry => collected.Owned.Contains((kind, entry.Id)));
    var sourced = all.Count(entry => entry.Source != Source.Unknown);
    Console.WriteLine($"  {kind.Label(),-16}  {all.Count} known, {owned} owned, {sourced} with a source");
}

// The thing most worth eyeballing: is the source text actually arriving?
var example = collected.Collectibles.FirstOrDefault(entry => entry.Kind == Kind.Mount && entry.Source == Source.Drop);
if (example is not null)
{
    Console.WriteLine($"  example mount     {example.Name} — {(example.Description ?? "(no text)").Replace("\n", " · ")}");
}

// -- the per-character files --------------------------------------------

var files = Files.CharacterFiles(wow, account, AddonWatch.Addon);
Console.WriteLine($"\nCHARACTERS ({files.Count} file(s))");

var characters = new List<CollectedCharacter>();
foreach (var file in files)
{
    byte[] bytes;
    try
    {
        bytes = File.ReadAllBytes(file);
    }
    catch (Exception error) when (error is IOException or UnauthorizedAccessException)
    {
        Console.WriteLine($"  {file} — could not read the file: {error.Message}");
        continue;
    }
    var character = Collector.ReadCharacter(bytes);
    if (character.IsOk)
    {
        var got = character.Value;
        var itemLevel = got.Detail.ItemLevel?.ToString() ?? "?";
        Console.WriteLine($"  {got.Character.FullName(),-24} level {got.Character.Level,-3} ilvl {itemLevel,-5} {got.Quests.Count} quests, {got.Detail.Professions.Count} professions");
        characters.Add(got);
    }
    else
    {
        Console.WriteLine($"  {file} — {character.Error}");
    }
}

if (characters.Count == 0)
{
    Console.WriteLine("\nNo character files yet. Log in on someone and log out.");
    return 0;
}

// -- what a run would look like ------------------------------------------

var cohort = new Cohort(characters.Select(got => got.Character.Key));

var inputs = new Inputs
{
    Progress = collected.Progress(),
    Attributions = new Dictionary<long, CharacterKey>(collected.EarnedBy),
    Criteria = new Dictionary<long, CriterionKind>(collected.Criteria),
    Owned = collected.Owned.Select(entry => entry.Id).ToHashSet(),
};
foreach (var got in characters)
{
    inputs.Primary[got.Character.Key] = got.Primary();
}

var baseline = Planner.TakeBaseline(inputs.Progress, [], DateTimeOffset.UtcNow);
var goals = Planner.Plan(baseline, cohort, inputs);
var run = new Run
{
    Name = "Inspection",
    Baseline = baseline,
    Cohort = cohort,
    Goals = goals,
};

var progress = run.Progress();
Console.WriteLine("\nA RUN ENROLLING EVERY CHARACTER SEEN");
Console.WriteLine($"  goals            {run.Goals.Count}");
Console.WriteLine($"  settled          {progress.Done}");
Console.WriteLine($"  poisoned         {run.Poisoned().Count()}");
Console.WriteLine($"    observable     {InBucket(run, goal => goal.Bucket is Bucket.Observable)}");
Console.WriteLine($"    attestable     {InBucket(run, goal => goal.Bucket is Bucket.Attestable)}");
Console.WriteLine($"  excluded         {progress.Excluded}");

var unattributed = run.Goals.Count(goal => goal.Standing is Standing.Poisoned { By: null });
Console.WriteLine($"  unattributed     {unattributed}");

// Why so few observable? Because one unmeasurable leaf makes a whole tree
// unmeasurable, and most criteria types have no per-character source at all.
// The variant, not its payload: "Quest", not "Quest(5000)".
var kinds = collected.Criteria.Values
    .GroupBy(kind => kind.Type.ToString())
    .Select(group => (Name: group.Key, Count: group.Count()))
    .OrderByDescending(entry => entry.Count)
    .ToList();

Console.WriteLine("\nWHAT THE CRITERIA MEASURE");
foreach (var (name, count) in kinds.Take(6))
{
    Console.WriteLine($"  {name,-20} {count}");
}
Console.WriteLine();
foreach (var line in new[]
{
    "Only quest- and achievement-backed criteria can be measured against one",
    "character's own data. Everything else — creature kills, exploration, spell",
    "casts — WoW records account-wide only, so a goal containing one of those",
    "goes to attestation rather than to a progress bar.",
})
{
    Console.WriteLine($"  {line}");
}

// The payoff, if there is one: poisoned goals with real movement on them.
var moving = run.Goals
    .Where(goal => goal.Standing.IsPoisoned && goal.Bucket is Bucket.Observable)
    .Where(goal => goal.Evaluation is { Observable: true, Progress: > 0 })
    .Select(goal => (Goal: goal, Evaluation: goal.Evaluation!.Value, Remaining: Math.Max(0, goal.Evaluation!.Value.Required - goal.Evaluation!.Value.Progress)))
    .OrderBy(entry => entry.Remaining)
    .ThenBy(entry => entry.Goal.AchievementId)
    .ToList();

Console.WriteLine($"\nCLOSEST TO DONE ({moving.Count} with progress)");
foreach (var (goal, evaluation, remaining) in moving.Take(10))
{
    Console.WriteLine($"  achievement {goal.AchievementId,-7} {evaluation.Progress}/{evaluation.Required} — {remaining} to go");
}
return 0;

static double Percent(int part, int whole) => whole == 0 ? 0.0 : part / (double)whole * 100.0;

static int InBucket(Run run, Func<Goal, bool> want) =>
    run.Goals.Count(goal => goal.Standing.IsPoisoned && want(goal));
