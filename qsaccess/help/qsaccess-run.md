# Running a request with qsaccess run

A saved transfer request can be run from a command line or a script, without opening its window:

```
qsaccess run <file.qdx>
```

The request is a `.qdx` file saved with **Save Request...** in a Data Transfer window or in the Integrated File System's transfer options. See [Transfer requests](data-transfer.md#transfer-requests). The command takes one file. It runs both types of request: a table's rows and files.

## With the program running

If QS/400 Access for Desktop is already running under your user, the command hands it the path of the request file and nothing else. The running program does the transfer:

- It uses its own session on the request's system. If it has none, it shows the Sign On dialog first.
- The transfer has a connection of its own, so the program's other windows stay usable.
- The program shows a progress window with **Cancel**.
- It sends the progress, the summary and the exit code back to the command.

The session never leaves the running program. If the command is ended while the transfer runs, for example by closing its console window, the running program cancels the transfer at its next progress line.

## Without the program running

With no running program, the command shows the Sign On dialog itself, runs the transfer with a progress window, and signs off at the end.

## Progress and summary

Progress goes to the standard error stream and the summary to the standard output stream, so a script can keep them apart.

- A table request reports the rows moved and the time taken as it goes. Its summary is [QAD7030](messages-reference.md#QAD7030), [QAD7031](messages-reference.md#QAD7031) or [QAD7032](messages-reference.md#QAD7032). When an upload stopped with rows kept, [QAD7033](messages-reference.md#QAD7033) follows with the row to resume from.
- A files request names each file as it starts. Its summary is [QAD8020](messages-reference.md#QAD8020) with the number of files and their size, or one [QAD8021](messages-reference.md#QAD8021) for each file that failed, or [QAD8022](messages-reference.md#QAD8022) when cancelled.

## Existing files

Nobody is there to answer a question, so a run replaces an existing file even when the request says to ask. This holds for a table download's output file and for each file of a files request. A request that says to skip, to append or to replace only if newer does as it says.

## Exit codes

The exit codes are the same for both types of request.

| Code | Meaning |
| --- | --- |
| 0 | The transfer is done. For files: every file. |
| 1 | The arguments are wrong, the request file cannot be read, or it names a system that is not configured here. |
| 2 | Sign-on was cancelled or failed, or the system's QAP version is too old: a table request needs version 4, a files request version 5. |
| 3 | The transfer failed. For a table, the summary says where and gives the row to resume from. For files, the summary names each file that failed; the other files are done. |
| 4 | The transfer was cancelled. |

## The per-user endpoint

The command reaches the running program through an endpoint that belongs to your user alone:

- On Windows it is a named pipe that refuses remote clients and that only the current user can open.
- On Linux it is a socket that only its owner can read and write.

Each end also checks which user is at the other end before anything is read. A connection from another user is closed unread and logged as [QAD8040](messages-reference.md#QAD8040). A command that finds another user's program at the endpoint prints the same message and ends with exit code 3.

Only the request file's path goes over the endpoint. Passwords and session tokens never do.

## Waiting for the command on Windows

On Windows `qsaccess` is a windowed program, so `cmd.exe` does not wait for it to end. To wait, and then read the exit code, start it this way in `cmd.exe`:

```
start /wait "" qsaccess run file.qdx
echo %ERRORLEVEL%
```

In PowerShell:

```
(Start-Process qsaccess -ArgumentList 'run','file.qdx' -Wait -PassThru).ExitCode
```

On Linux the shell waits for the command as for any other.

## Why there are no fully unattended runs

The program never saves passwords, in a request file or anywhere else. A run therefore needs a session that someone signed on to: either the running program's, or one made in the Sign On dialog the command shows. A run with no running program signed on to the request's system always asks for a password, and ends with exit code 2 if the dialog is cancelled.

To run requests from a script without a prompt each time, keep the program running and signed on to the system. Each run then uses that session.

## Request file keys for a files request

A request file is text. Its `[qsaccess]` table says that it is a transfer request and gives its format version. In version 2 the `[transfer]` table has `type = "table"` or `type = "files"`; a version 1 file is always a table's. Both types name the `system`.

A files request has these keys in `[transfer]`:

| Key | Meaning |
| --- | --- |
| `direction` | `download` or `upload`. |
| `local` | For an upload: the files, folders and name patterns to send, such as `data/*.csv`. For a download: the folder to write into. |
| `remote` | For an upload: the folder on the system. For a download: a folder, a file, or a folder and a name pattern, such as `/home/BOB/reports/*.txt`. It starts with `/`. |
| `recurse` | Whether folders are taken with all the folders in them. The default is `true`. |
| `data` | `binary` (the default) or `text`. |
| `ccsid` | The CCSID of files an upload makes on the system. The default is 1208. |
| `line_endings` | For text only: `crlf` or `lf` on this computer. |
| `existing` | What to do with a file that already exists: `ask`, `skip`, `replace` or `replace-if-newer`. |

A `local` path that is not absolute is taken from the folder the request file is in.

## See also

- [Data Transfer and transfer requests](data-transfer.md)
- [Integrated File System, transfers and the editor](integrated-file-system.md)
- [Signing on and passwords](signing-on.md)
- [Installing, repairing and uninstalling](installing.md)
- [Messages reference](messages-reference.md)
