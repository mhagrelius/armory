using System.Collections.Concurrent;
using Armory.Store;

namespace Armory.Client.Shell;

/// <summary>
/// The one thread that owns the store.
/// </summary>
/// <remarks>
/// Recording is a flag in the database, not a property of a connection, so a
/// second thread applying a pull would silence the first thread's writes for
/// as long as it took and neither would know. Every read and every write
/// goes through here, in order, and the network never touches the store at
/// all: it is handed a parcel and gives back an answer.
/// </remarks>
public sealed class StoreWorker : IDisposable
{
    private readonly BlockingCollection<Action> queue = [];
    private readonly Thread thread;
    private readonly Store.Store store;

    public StoreWorker(Store.Store store)
    {
        this.store = store;
        thread = new Thread(Run) { Name = "armory-store", IsBackground = true };
        thread.Start();
    }

    /// <summary>Do one thing with the store, on its thread, and hand the answer back.</summary>
    public Task<T> On<T>(Func<Store.Store, T> work)
    {
        var done = new TaskCompletionSource<T>(TaskCreationOptions.RunContinuationsAsynchronously);
        queue.Add(() =>
        {
            try
            {
                done.SetResult(work(store));
            }
            catch (Exception error)
            {
                done.SetException(error);
            }
        });
        return done.Task;
    }

    public Task On(Action<Store.Store> work) => On(store =>
    {
        work(store);
        return true;
    });

    private void Run()
    {
        foreach (var work in queue.GetConsumingEnumerable())
        {
            work();
        }
    }

    public void Dispose()
    {
        queue.CompleteAdding();
        thread.Join();
        store.Dispose();
        queue.Dispose();
    }
}
