# Messages

**Messages** lists a message queue of a system, replies to inquiries, removes and sends messages, and tells you when a message arrives. Choose **Messages** in the **General** group of the main window's task pane. If the program has no session on the selected system, it asks you to sign on first (see [Signing on](signing-on.md)). A Messages window that is already open for the system comes to the front.

Messages needs QAP version 6. For a system with an older version the task is dimmed, and the reason is [QAD5001](messages-reference.md#QAD5001).

## Choosing a queue

**Queue** at the top of the window has:

- **My messages**, with your user profile's name: your own message queue. The window opens on it.
- **System operator (QSYSOPR)**, offered only when your user profile may read that queue.
- **Other queue...**, which asks for a message queue as `LIBRARY/NAME`. A name alone is looked for in the library list. The queue then stays in the list while the window is open.

## The list

The list shows the queue newest first, with the columns **Sent**, **Type**, **Severity**, **ID**, **Message**, **From user** and **From job**. **Message** is the first line of the message's text.

An inquiry that is waiting for a reply is in bold.

The list holds 100 messages a page. **Previous** and **Next** move between pages, and the line beside them says which messages and which page are shown. The status bar shows [QAD9001](messages-reference.md#QAD9001) with the number of messages on the queue. Select a column heading to sort the page shown. **View > Refresh** (`F5`) reads the page again.

While the queue is watched (see below), a new message appears at the top of the first page as it arrives, and a reply marks the inquiry it answers as answered.

## Reading a message

Select a message. The pane under the list shows it whole: its text, its second-level text as the system sent it, its type, ID and severity, when it was sent, and by which user and job. For an inquiry it also says **Waiting for a reply.** or shows the answer.

- A reply has the button **Go to the Message It Answers**. An answered inquiry or sender's copy has **Go to Its Reply**. The program finds the other message, on another page if need be, and selects it.
- Double-click a message, or choose **Messages > Details...**, to open it in the message details dialog.

## Replying to an inquiry

1. Select the inquiry.
2. Type the reply in **Reply (empty for default)**. Leave it empty to send the message's default reply.
3. Select **Reply** or press `Enter`.

The status bar shows [QAD9002](messages-reference.md#QAD9002), and the inquiry is no longer bold. If the message does not take the reply you typed, the system's own message says so and the inquiry stays waiting.

## Removing messages

- **Messages > Remove** (`Delete`) removes the selected messages after asking, with the count.
- If any of them is an inquiry not answered yet, a separate confirmation, **Remove Unanswered Inquiries**, asks again: the sender of such an inquiry gets no reply, and a program waiting for one goes on waiting.
- **Messages > Remove All Answered and Informational...** removes every message on the queue that is not waiting for a reply. The confirmation gives the count and says how many waiting messages stay.

After a removal the status bar shows [QAD9003](messages-reference.md#QAD9003).

## Send Message

Choose **File > Send Message...** (`Ctrl+N`).

1. Under **To**, choose **User profile** and type a profile, or **Message queue** and type a queue as `LIBRARY/NAME`.
2. Type the text in **Message**. A message takes up to 512 characters.
3. To ask for an answer, tick **Inquiry (wait for a reply)**.
4. Select **Send**.

The dialog shows [QAD9004](messages-reference.md#QAD9004). For an inquiry it then says that it is waiting for the reply, and shows the reply and who sent it when it comes. If you close the dialog, the reply still arrives on your own queue.

## Watching queues

While you are signed on to a system with QAP version 6, the program watches your own message queue there, whether or not a Messages window is open. The system operator's queue is watched too when you choose it in Options.

### Message Arrived

When a message arrives on a watched queue and the queue's option asks for it, the **Message Arrived** window opens. By default only inquiries open it. A reply and a sender's copy never do.

- The window opens without taking the keyboard from the window you are typing in. Its taskbar entry flashes instead; on X11 the window is marked urgent.
- It shows the message, its second-level text and who sent it. For an inquiry, type the answer in **Reply (empty for default)** and select **Reply**. An inquiry answered in the meantime, here or elsewhere, shows its answer instead.
- Several arrivals wait in the one window: **Previous** and **Next** move between them, and a count such as `2 of 3` shows where you are.
- **Open Messages** opens the Messages window for that system. **Close** closes the window and empties it.

### Options

**Messages > Options...** sets, for this system:

- **My messages**: **When a message arrives** is **Inquiries only** (the default), **All messages** or **None**. It decides what opens Message Arrived. Your own queue is always watched.
- **System operator (QSYSOPR)**: **Watch the system operator's messages**, off by default, and its own **When a message arrives**. When your user profile may not display that queue's messages, the dialog says so and the box cannot be ticked.

**OK** applies the choice at once. If the system refuses to let you watch the operator's queue, the option is turned off again with [QAD9012](messages-reference.md#QAD9012).

### Messages waiting

While the selected system has watched queues, the main window's status bar shows **Messages waiting:** and the number of inquiries waiting for a reply on them. Click it to open Messages.

### After a dropped connection

When the connection is lost and resumed, or you sign on again, the program watches the queues again by itself and lists the messages that arrived in between, each once. The main window's status bar shows [QAD9013](messages-reference.md#QAD9013) with their number, and an inquiry among them opens Message Arrived as usual. If a queue cannot be watched again, [QAD9011](messages-reference.md#QAD9011) gives the reason; sign off and on again once it is corrected.

## See also

- [Signing on and passwords](signing-on.md)
- [Run Command and prompting](run-command.md)
- [Printer Output and the viewer](printer-output.md)
- [Keyboard reference](keyboard-reference.md)
- [Messages reference](messages-reference.md)
