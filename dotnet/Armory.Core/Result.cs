namespace Armory;

/// <summary>
/// An expected failure as a value. Exceptions are for the seams (SQLite,
/// HTTP, the file system) and are turned into one of these there.
/// </summary>
public readonly record struct Result<T, TError>
{
    private readonly T? value;
    private readonly TError? error;

    private Result(T? value, TError? error, bool isOk)
    {
        this.value = value;
        this.error = error;
        IsOk = isOk;
    }

    public bool IsOk { get; }

    public T Value => IsOk ? value! : throw new InvalidOperationException($"not ok: {error}");

    public TError Error => IsOk ? throw new InvalidOperationException("ok") : error!;

    public static Result<T, TError> Ok(T value) => new(value, default, true);

    public static Result<T, TError> Err(TError error) => new(default, error, false);

    public TResult Match<TResult>(Func<T, TResult> ok, Func<TError, TResult> err) =>
        IsOk ? ok(value!) : err(error!);

    public Result<TNext, TError> Map<TNext>(Func<T, TNext> map) =>
        IsOk ? Result<TNext, TError>.Ok(map(value!)) : Result<TNext, TError>.Err(error!);

    public Result<TNext, TError> Then<TNext>(Func<T, Result<TNext, TError>> next) =>
        IsOk ? next(value!) : Result<TNext, TError>.Err(error!);
}

/// <summary>The value of a result that carries nothing but success.</summary>
public readonly record struct Unit
{
    public static Unit Value => default;
}
