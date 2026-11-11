# Signing on and passwords

Every task except a 5250 session works through a session on the system, which starts when you sign on. Sign-on, password changes and those tasks use the system's QAP port, always over TLS.

## When the program signs on

The program signs on to a system once each time it runs.

- The first task that needs the system shows the Sign On dialog. So does **Actions > Sign On...**, for the system selected in the task pane.
- Every task after that shares the session. Opening a second window on the same system does not ask again.
- The program asks again only when the session has ended. See [When a session ends](signing-on.md#when-a-session-ends).

Once you are signed on, the status bar shows `name: Signed on as PROFILE`. If you choose **Actions > Sign On...** while signed on, the status bar says so with [QAD4020](messages-reference.md#QAD4020) and nothing else happens.

## The Sign On dialog

1. Type your **User ID**. The field starts with the **Default user ID** of the system configuration, if one is set.
2. Type your **Password**.
3. Select **Sign On**.

The system applies the same checks as its 5250 sign-on display: the limit of sign-on attempts, disabled profiles and expired passwords. Each attempt is audited on the system as a display sign-on is.

When the system refuses the sign-on, its own message is shown as it sent it, with its message ID. Select **Details...** for the rest of the message. The dialog stays open so that you can try again. The program's log records the refusal as [QAD4021](messages-reference.md#QAD4021).

Other things that can stop a sign-on:

- The system's certificate is not trusted. The **Certificate Not Trusted** dialog opens; **Trust for this system** adds the certificate to the system's trusted list and the sign-on goes on. Otherwise the dialog reports [QAD1011](messages-reference.md#QAD1011).
- The QAP server cannot be reached ([QAD4053](messages-reference.md#QAD4053)) or TLS cannot be set up ([QAD4054](messages-reference.md#QAD4054)). [Verify Connection](verify-connection.md) shows which step fails.

## Passwords are never saved

The program never saves a password and never writes one to its log. A password field is cleared as soon as it is read. A system configuration keeps only the **Default user ID**.

## An expired password

When the password is correct but has expired, the program shows [QAD4022](messages-reference.md#QAD4022) with the system's message.

1. Select **Change Password...**. The Change Password dialog opens with the user ID and the current password filled in.
2. Type the new password twice and select **OK**.

The system changes the password and signs you on in one step. **Cancel** leaves you not signed on.

## Password expiry warning

After each sign-on, and each time a session is resumed on a new connection, the program asks the system when the password expires. When that is within the system's warning period, it shows [QAD4031](messages-reference.md#QAD4031) with the number of days left.

- **Change Password...** opens the Change Password dialog.
- **Close** leaves the password as it is.

The warning is shown once per session. A password that never expires shows nothing, and neither does a system whose QAP version does not report password status.

## Change Password

Select **Actions > Change Password...** for the system selected in the task pane.

- **User ID**: when you are signed on, this is the signed-on profile and cannot be changed. When you are not, type the user ID; the program changes its password and signs on.
- **Current password**
- **New password**
- **Confirm new password**

Select **OK**. When the two new passwords differ, nothing is sent and the dialog shows [QAD4027](messages-reference.md#QAD4027). The system's password rules decide whether the new password is accepted; a refusal is shown as the system sent it. When the change is accepted, the program reports [QAD4023](messages-reference.md#QAD4023). Use the new password from then on, including at the 5250 sign-on display.

## Sign Off

**Actions > Sign Off** ends the session on the selected system, for every window that uses it, and reports [QAD4028](messages-reference.md#QAD4028). The next task on that system asks you to sign on again.

Closing a task window never signs off. Closing the program without signing off leaves the session on the system until the system's idle timeout ends it.

## When a session ends

| What happens | What the program does |
|---|---|
| The connection closes or drops while the session is still good | It reconnects and resumes the session without asking. A request that was cut off is sent again only if it changes nothing; otherwise its window says the connection was lost and you decide. |
| The session can no longer be resumed, for example after the system's idle timeout | It shows [QAD4026](messages-reference.md#QAD4026) with **Sign On...** and **Close**. |
| An operator ends the session | It shows [QAD4024](messages-reference.md#QAD4024) with the system's message, **Sign On...** and **Close**. |
| The QAP server ends | It shows [QAD4025](messages-reference.md#QAD4025) with the system's message, **Reconnect** and **Close**. Every session on the system ends with the server, and nothing reconnects by itself. **Reconnect** connects again and then shows the Sign On dialog. |

**Sign On...** opens the Sign On dialog. A window that has its own connection, such as Run Command or Run SQL, shows the system's message itself when its session ends and offers **Sign On...** too.

After a resume or a new sign-on, the program goes on watching for new messages on the systems that report them, and lists what arrived in the meantime.

## See also

- [Getting started](getting-started.md)
- [Verify Connection](verify-connection.md)
- [Certificate Management](certificate-management.md)
- [5250 Display](display-5250.md)
- [Messages](messages.md)
- [Messages reference](messages-reference.md)
