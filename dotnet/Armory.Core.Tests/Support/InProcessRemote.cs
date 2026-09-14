using Armory.Sharing;
using Armory.Store;

namespace Armory.Tests.Support;

/// <summary>The Rust server's routes, answered by a store in the same process. What the sync check does with a socket, done here without one.</summary>
internal sealed class InProcessRemote : IRemote
{
    private readonly Armory.Store.Store server;
    private readonly string machine;

    public InProcessRemote(Armory.Store.Store server, string machine)
    {
        this.server = server;
        this.machine = machine;
    }

    public Result<Applied, SyncError> Push(Parcel parcel) =>
        server.Apply(parcel, Recording.As(machine)).Match(Result<Applied, SyncError>.Ok, error => Result<Applied, SyncError>.Err(new SyncError(error.Message)));

    public Result<Pulled, SyncError> Pull(long since, int limit) =>
        server.LogSince(since, machine, limit).Match(Result<Pulled, SyncError>.Ok, error => Result<Pulled, SyncError>.Err(new SyncError(error.Message)));

    public Result<bool, SyncError> Wait(long since) => Result<bool, SyncError>.Ok(false);
}
