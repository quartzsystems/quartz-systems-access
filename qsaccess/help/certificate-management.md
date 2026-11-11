# Certificate Management

Certificate Management lists every certificate that any system configuration trusts, and lets you look at them, add to them and take them away. Select **Tools > Certificate Management...**, or **Certificate Management** in the **Management** group of the task pane.

## What a certificate is trusted for

Connections that use TLS check the certificate the system presents: every QAP connection, and 5250 display and printer sessions when **Use TLS for all connections** is selected for the system. A system's certificate is trusted when one of these holds:

- It was issued by a certificate authority in the operating system's certificate store. See [This computer](#this-computer).
- It was issued by a certificate authority in the system configuration's own list of trusted certificates.
- The certificate itself is in that list. It is then trusted as it stands, which is how a self-signed certificate is trusted.

Each system configuration has its own list. A certificate trusted for one system is not trusted for another unless it is in that system's list too. There is no setting that turns the check off.

Only the system proves who it is with a certificate. You prove who you are by signing on with a user ID and password, so client certificates are not used: neither QAP nor Telnet TLS asks this computer for a certificate or a private key, and there are none to manage here. See [Signing on and passwords](signing-on.md).

## The Certificates tab

The list has one row for each certificate, however many systems trust it.

| Column | What it shows |
|---|---|
| **Subject** | Who the certificate was issued to. |
| **Issuer** | Who issued it. For a self-signed certificate this is the same as the subject. |
| **Valid from** | The first day the certificate is valid. |
| **Valid to** | The last day it is valid. A certificate with 30 days or fewer left shows the date and the number of days left. A certificate that has expired shows **Expired** in red. |
| **SHA-256 fingerprint** | The fingerprint that identifies this exact certificate. Compare it with the fingerprint shown on the system itself before you trust a certificate. |
| **Trusted for** | The systems whose configurations trust it. |

Dates and times are in UTC. Drag the edge of a column heading to make a column wider.

A system configuration can also refer to a certificate file instead of holding the certificate. If such a file cannot be read, or holds no certificate, the list shows a row with the file's path and the reason in red, so that the reference can be removed.

## View

Select a certificate and select **View...**, or double-click it. The dialog shows every field, and below them the certificate as PEM text. **Copy** puts all of it on the clipboard as text.

| Field | What it shows |
|---|---|
| **Subject** and **Issuer** | Who the certificate was issued to, and by whom. |
| **Alternative names** | The host names and addresses the certificate is good for, for example `DNS:sys1.example.net`. A connection checks the system's name against these. |
| **Serial number** | The number the issuer gave it. |
| **Valid from** and **Valid to** | The times it is valid between, in UTC. |
| **Public key** | The kind and size of its key, for example `EC P-256` or `RSA 2048 bits`. |
| **Signature** | How the issuer signed it. |
| **Certificate authority** | **Yes** when the certificate may issue others, **No** when it says it may not, **Not stated** when it does not say. |
| **SHA-256 fingerprint** | The fingerprint that identifies this exact certificate. |
| **Trusted for** | The systems whose configurations trust it. |

Rest the pointer on a value that is too long for its line to see all of it.

## Import

**Import...** adds a certificate from a file.

1. Choose the file. It may be a PEM file, with one certificate or several, or a DER file with one certificate.
2. Tick the systems that are to trust it, then select **OK**. At least one system must be ticked.

Each certificate in the file is added to the list of every system you ticked. A system that already trusts a certificate is left as it is.

- A file with no certificate in it is refused with [QAD0043](messages-reference.md#QAD0043). A private key or a certificate request is not a certificate.
- A certificate that has expired is reported with [QAD0044](messages-reference.md#QAD0044). Select **Import** to trust it all the same, or **Skip** to leave it out.
- When a certificate is imported, [QAD0040](messages-reference.md#QAD0040) says for how many systems.

You do not have to import a certificate before you connect. When a system presents a certificate that is not trusted, the Certificate Not Trusted dialog shows it, and **Trust for this system** adds it to that system's list ([QAD1012](messages-reference.md#QAD1012)). It then appears here.

## Export

Select a certificate and select **Export...** to write it to a PEM file. [QAD0042](messages-reference.md#QAD0042) names the file. The file holds the certificate only. A certificate contains no private key, so the file is safe to pass on.

## Trusted for

Select a certificate and select **Trusted for...** to change which systems trust it. Tick the systems that are to trust it and clear the others, then select **OK**. Clearing every system removes the certificate from the list.

## Remove

Select a row and select **Remove**. The confirmation names the systems that will stop trusting the certificate. After **Remove**, [QAD0041](messages-reference.md#QAD0041) says how many systems it was removed from.

- A connection to one of those systems that needs the certificate is refused as not trusted from then on. Connections that are already open stay open.
- When a system's list holds the certificate in a file it refers to, the file is not changed. The reference is replaced by the file's other certificates, kept in the configuration, so the others stay trusted.

## The Security tab of a system's properties

The **Security** tab of System Properties shows the same list for one system: each certificate that system's configuration trusts, by its subject and Valid to date, with **Add...**, **Remove** and **View...**. Certificate Management and the Security tab edit the same lists, and each shows a change made in the other at once.

While System Properties is open for a system, the changes made to that system's list, on the Security tab or here, are kept or dropped with the dialog: **OK** saves them and **Cancel** drops them. Changes to the lists of other systems are saved as they are made. See [System Configurations](getting-started.md#system-configurations).

## This computer

The **This computer** tab says how many certificates the operating system's certificate store supplies. They are trusted for every system, beside the certificates on the **Certificates** tab.

The store is managed by the operating system, with its own tools. Nothing in QS/400 Access for Desktop changes it. The program reads the store when it first makes a TLS connection, so a change to the store takes effect the next time the program starts.

## See also

- [Getting started](getting-started.md)
- [Signing on and passwords](signing-on.md)
- [Verify Connection](verify-connection.md)
- [5250 Display](display-5250.md)
- [Messages reference](messages-reference.md)
