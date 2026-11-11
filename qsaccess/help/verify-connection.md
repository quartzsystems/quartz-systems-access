# Verify Connection

Verify Connection tests the path from this computer to a system, one step at a time, and shows where it stops. Use it after adding a system configuration, or when a session or a sign-on fails.

## Running the checks

1. Select **Tools > System Configurations...**.
2. Select the system and select **Verify...**. The **Verify Connection** dialog opens with the checks listed and no results.
3. To include a test sign-on, type a **User ID** and **Password** under **Test sign-on (optional)**. To leave it out, clear the **User ID** field. Read [The test sign-on](verify-connection.md#the-test-sign-on) first.
4. Select **Verify**.

The checks run in order and each row is filled in as its result is known. **Verify** is disabled until they are all done. Select **Verify** again to run them again.

The list has three columns: **Check**, **Result** and **Detail**.

## What the results mean

| Result | Meaning |
|---|---|
| **OK** | The check passed. **Detail** says what was found. |
| **Failed** | The check did not pass. **Detail** gives the message. Double-click the row for the whole message, with its cause and recovery; when the system sent a message of its own, that is shown as sent. |
| **Skipped** | The check was not run. **Detail** says why: it does not apply to this system, or a check it depends on failed. |

## The checks, in order

| Check | What it tests | When it fails |
|---|---|---|
| **Name lookup** | The system name can be turned into an address. When **Look up address** is **Never**, the fixed IP address is used instead. **Detail** shows the addresses found. | [QAD1003](messages-reference.md#QAD1003). Every other check is skipped. |
| **Telnet port** | The Telnet port accepts a connection. | [QAD1002](messages-reference.md#QAD1002) when the connection is refused, otherwise [QAD1004](messages-reference.md#QAD1004). The QAP checks still run. |
| **Telnet TLS port** | The Telnet TLS port accepts a connection, TLS can be set up, and the certificate is trusted. Skipped when **Use TLS for all connections** is not selected for the system. | [QAD1002](messages-reference.md#QAD1002) or [QAD1004](messages-reference.md#QAD1004) for the connection, [QAD1010](messages-reference.md#QAD1010) for TLS, [QAD1011](messages-reference.md#QAD1011) for the certificate. The QAP checks still run. |
| **QAP port** | The QAP port accepts a connection. | [QAD4053](messages-reference.md#QAD4053). The QAP server may not be started, or may be on another port. The checks after it are skipped. |
| **QAP TLS 1.3 and certificate** | TLS 1.3 can be set up with the QAP server, and its certificate is trusted. **Detail** shows the protocol and cipher. | [QAD4054](messages-reference.md#QAD4054) for TLS (the server accepts TLS 1.3 only), [QAD1011](messages-reference.md#QAD1011) for the certificate. The checks after it are skipped. |
| **QAP Hello** | The program and the server agree on a QAP version. **Detail** shows the version, the system's name and its release. | [QAD4055](messages-reference.md#QAD4055) when no version is shared, otherwise [QAD4060](messages-reference.md#QAD4060). The test sign-on is skipped. |
| **Test sign-on** | The user ID and password are accepted. Skipped when no user ID is entered. | [QAD4022](messages-reference.md#QAD4022) for an expired password; otherwise [QAD4021](messages-reference.md#QAD4021), with the system's own message in **Detail**. |

The port checks show the port number from the system configuration in their names, for example **QAP port 2320**. To change a port, see the **Ports** tab in [Getting started](getting-started.md#system-properties).

## The overall result

When every check has a result, the line at the bottom of the dialog shows one of these:

| Message | Meaning |
|---|---|
| [QAD4050](messages-reference.md#QAD4050) | Every check that ran passed. |
| [QAD4051](messages-reference.md#QAD4051) | The connection works, but the test sign-on failed. |
| [QAD4052](messages-reference.md#QAD4052) | At least one check other than the test sign-on failed. |

## A certificate that is not trusted

When a check fails because the system's certificate is not trusted, the **Certificate Not Trusted** dialog opens once the checks are done. Compare the certificate with the one on the system. If you select **Trust for this system**, the certificate is added to the system's trusted list and the checks run again by themselves. The Telnet TLS port and the QAP server can present different certificates; each is asked about once.

## The test sign-on

The test sign-on is optional, and it is a real sign-on.

- It signs on with the user ID and password entered, then signs off at once. **Detail** says so.
- It does not sign the program on. After a test that passes, the main window still shows the system as not connected until you sign on. See [Signing on and passwords](signing-on.md).
- A failed test counts toward the system's limit of sign-on attempts (system value `QMAXSIGN`). Reaching the limit can disable the profile.
- The system audits it like any other sign-on.
- The **User ID** field starts with the **Default user ID** of the system configuration. Clear it if you do not want a test sign-on.
- The password is never saved. The field is cleared as soon as the checks start, so type it again for another run.

When the result is [QAD4022](messages-reference.md#QAD4022), the password is correct but has expired. Change it from the Sign On dialog: see [An expired password](signing-on.md#an-expired-password).

## What to do when a check fails

| Failed check | Things to look at |
|---|---|
| **Name lookup** | The spelling of **System name** on the **General** tab, or a **Fixed IP address** on the **Connection** tab. |
| **Telnet port**, **Telnet TLS port** | Whether the Telnet server is started on the system, and the Telnet ports on the **Ports** tab. |
| **QAP port** | Whether the QAP server is started (`STRTCPSVR SERVER(*QAP)` on the system), the QAP port on the **Ports** tab, and any firewall in between. |
| **QAP TLS 1.3 and certificate**, **QAP Hello** | Whether the QAP port is really the QAP server's. |

## See also

- [Getting started](getting-started.md)
- [Signing on and passwords](signing-on.md)
- [Certificate Management](certificate-management.md)
- [Messages reference](messages-reference.md)
