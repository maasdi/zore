# Photo board

A small web application that uses the HTTP helpers from §37.16 to §37.18
together: a visitor picks a display name, uploads photos, and everyone sees
them on one page.

| File | Responsibility |
| --- | --- |
| `main.ore` | Startup, server limits, and routes |
| `handlers.ore` | Forms, cookies, uploads, validation, and file serving |
| `board.ore` | Mutex-protected list of photos |
| `page.ore` | The HTML page, with every value escaped |

Run `zore run examples/photo-board/main.ore` from a folder you can write to,
then open <http://127.0.0.1:8080>. Photos are stored in
`photo-board-data/photos` under the current folder. The list of photos lives
in memory, so that folder is emptied at every start; files from an earlier
run are never served or overwritten. From another terminal:

```sh
curl -i -c jar -d 'name=Ada Lovelace' http://127.0.0.1:8080/name
curl -i -b jar --form-string 'title=My cat' -F photo=@cat.png http://127.0.0.1:8080/photos
curl -i http://127.0.0.1:8080/photos/1.png
```

The limits are named constants at the top of `handlers.ore`. What each part
shows:

- **Forms and cookies.** `POST /name` reads a URL-encoded form with
  `Request.ReadForm`, checks the name, and stores it in a `board_name` cookie
  through `ResponseWriter.SetCookie` with `Path=/`, `HttpOnly`,
  `SameSite=Lax`, and a 30-day `Max-Age`. The value is escaped with
  `url.QueryEscape`, because cookie values cannot hold spaces.
  `POST /name/forget` sends the same cookie, built by the same helper, with
  `MaxAge` -1 to delete it. Every request reads the name back with
  `Request.Cookie` and checks it again.
- **Uploads.** `POST /photos` reads a multipart form with
  `Request.ReadMultipartForm`. Its limits allow 4 parts, 1 KiB of text, and
  one file of 5 MiB. The server's `Config.MaxBodyBytes` is raised to match,
  since its default is 1 MiB. An upload past any limit gets `413`; a missing
  title or photo gets `400`; a file whose extension is not `.png`, `.jpg`,
  `.jpeg`, `.gif`, or `.webp` gets `415`, so SVG and icon files, which browsers
  may treat as documents, are refused. The extension is only a label: the
  example does not check that the bytes really are an image, and it serves
  photos with `nosniff` so browsers keep to the label.
- **Storing.** The upload is copied into `photo-board-data/photos/<id><ext>`,
  a name the server chooses, never the client's file name. The file is
  created with `os.OpenExclusive`, so nothing existing is overwritten, and it
  is deleted again if the copy fails, before the photo is listed. The
  temporary upload file is deleted when the form is dropped at the end of the
  handler, also on errors.
- **File serving.** `GET /photos/{file}` uses `http.ServeFileIn`, so a request
  can only reach files directly inside the photo folder, with the right
  `Content-Type`, `Content-Length`, `Last-Modified`, and `nosniff`.
- **Redirects.** Successful posts answer `303 See Other` with `Location: /`,
  so reloading the page does not post the form again.

The cookie only remembers the name the visitor typed; it is not a login, and
anyone can pick any name. A real application would keep sessions on the
server, name them with a random token from a cryptographic source such as the
`crypto` library's `RandomBytes` (§37.9), and protect its forms against
cross-site requests. `SameSite=Lax` keeps other sites from sending the cookie
with their form posts, so they cannot upload in a visitor's name; but
`POST /name` needs no cookie, so another site could still post that form and
change the visitor's name. A token in each form, checked by the server, closes
that gap.

Press Ctrl-C (or send SIGTERM on Unix) to drain connections and stop.
`http.ListenAndServeWithConfig` handles the signals and graceful shutdown.
