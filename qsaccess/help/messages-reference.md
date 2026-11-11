# Messages reference

Every message of QS/400 Access for Desktop itself is listed here, in order of its ID. A message ID is `QAD` followed by four digits.

A message has a first line, which is what the status bar and message boxes show, and two more parts that say what happened and what to do:

- **Cause** says why the message was sent.
- **Recovery** says what to do about it, or that nothing needs doing.

`&1`, `&2` and so on stand for the names and values the message is sent with: a system's name, a file's path, a count.

The severity says how serious the message is:

| Severity | Meaning |
|---|---|
| 00 to 09 | Information. Something was done. |
| 10 to 19 | Warning. Something needs your attention, and work can go on. |
| 20 and up | Error. What was asked for was not done. |

Select **Details...** in a message box to see a message's cause and recovery. The message ID in the details opens the message here.

Messages whose IDs do not start with `QAD` come from the system: they are shown as the system sent them, and the system's own second-level text is their help. See [Messages](messages.md) for the messages in a system's message queues.

{{messages}}
