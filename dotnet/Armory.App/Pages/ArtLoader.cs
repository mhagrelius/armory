using System.Runtime.InteropServices;
using System.Runtime.InteropServices.WindowsRuntime;
using Armory.Client.Images;
using Microsoft.UI.Xaml.Media.Imaging;
using Windows.Storage.Streams;

namespace Armory.App.Pages;

/// <summary>
/// Bytes from the image cache, decoded at the size a cell draws. The window
/// keeps its own decoded pictures keyed by URL and size, because the same
/// render wanted small in a grid and large in a dialog is two different
/// pictures, and decoding is the expensive half.
/// </summary>
public static class ArtLoader
{
    private static readonly Lru<string, BitmapImage> Decoded = new(256);

    /// <summary>The picture, or null for art that cannot be had. Never throws: a tile that gets nothing keeps its placeholder, which already reads correctly.</summary>
    public static async Task<BitmapImage?> Decode(Images images, string url, int size)
    {
        var key = $"{url}@{size}";
        if (Decoded.Get(key) is { } held)
        {
            return held;
        }
        var bytes = await images.Load(url);
        if (bytes is null)
        {
            return null;
        }
        var picture = new BitmapImage { DecodePixelWidth = size };
        try
        {
            using var stream = new InMemoryRandomAccessStream();
            await stream.WriteAsync(bytes.AsBuffer());
            stream.Seek(0);
            await picture.SetSourceAsync(stream);
        }
        catch (COMException)
        {
            // Bytes that will not decode: a truncated file from an interrupted
            // launch. Dropped rather than kept or retried.
            images.Refuse(url);
            return null;
        }
        catch (ArgumentException)
        {
            images.Refuse(url);
            return null;
        }
        Decoded.Put(key, picture);
        return picture;
    }
}
