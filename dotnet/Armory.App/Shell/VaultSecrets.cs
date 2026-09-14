using Armory.Client.Shell;
using Windows.Security.Credentials;

namespace Armory.App.Shell;

/// <summary>
/// The Windows Password Vault as the secret store: the same service the GTK
/// build reaches through the freedesktop secrets portal, and the same
/// resource name, <c>us.hagreli.Armory</c>. A wrong resource name here reads
/// exactly like a vault with nothing in it.
/// </summary>
public sealed class VaultSecrets : ISecrets
{
    private readonly PasswordVault vault = new();

    public string? Get(string name)
    {
        try
        {
            var credential = vault.Retrieve(SecretNames.Resource, name);
            credential.RetrievePassword();
            return credential.Password;
        }
        catch (Exception)
        {
            // The vault throws for "not there", which is the ordinary case.
            return null;
        }
    }

    public void Set(string name, string value)
    {
        Clear(name);
        vault.Add(new PasswordCredential(SecretNames.Resource, name, value));
    }

    public void Clear(string name)
    {
        try
        {
            vault.Remove(vault.Retrieve(SecretNames.Resource, name));
        }
        catch (Exception)
        {
            // Nothing to clear.
        }
    }
}
