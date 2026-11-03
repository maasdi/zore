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
`photo-board-data/photos` under the current folder; the list of photos lives
in memory and starts empty on every run. From another terminal:

```sh
curl -i -c jar -d 'name=Ada Lovelace' http://127.0.0.1:8080/name
curl -i -b jar --form-string 'title=My cat' -F photo=@cat.png http://127.0.0.1:8080/photos
curl -i http://127.0.0.1:8080/photos/1.png
```

What each part shows:

- **Forms and cookies.** `POST /name` reads a URL-encoded form with
  `Request.ReadForm(1024)`, checks the name, and stores it in a `board_name`
  cookie through `ResponseWriter.SetCookie` with `Path=/`, `HttpOnly`,
  `SameSite=Lax`, and a 30-day `Max-Age`. The value is escaped with
  `url.QueryEscape`, because cookie values cannot hold spaces. `POST
  /name/forget` sends the same cookie with `MaxAge` -1 to delete it. Every
  request reads the name back with `Request.Cookie` and checks it again.
- **Uploads.** `POST /photos` reads a multipart form with
  `Request.ReadMultipartForm`. Its limits allow 4 parts, 1 KiB of text, and one
  file of 5 MiB. The server's `Config.MaxBodyBytes` is raised to match, since
  its default is 1 MiB. An oversized upload gets `413`; a missing title or
  photo gets `400`; a file whose extension is not a PNG, JPEG, GIF, or WebP
  image gets `415`. SVG is refused because it can carry scripts. The upload
  is copied into `photo-board-data/photos/<id><ext>`; the name is chosen by
  the server, never taken from the client's file name. The temporary file is
  deleted when the form is dropped at the end of the handler, also on errors.
- **File serving.** `GET /photos/{file}` uses `http.ServeFileIn`, so a request
  can only reach files directly inside the photo folder, with the right
  `Content-Type`, `Content-Length`, `Last-Modified`, and `nosniff`.
- **Redirects.** Successful posts answer `303 See Other` with `Location: /`,
  so reloading the page does not post the form again.

The cookie only remembers the name the visitor typed; it is not a login, and
anyone can pick any name. A real application would keep sessions on the
server and identify them with a random token from a cryptographic source,
and would protect its forms against cross-site requests. `SameSite=Lax` already
keeps other sites from sending the cookie with their form posts.

Press Ctrl-C (or send SIGTERM on Unix) to drain connections and stop.
`http.ListenAndServeWithConfig` handles the signals and graceful shutdown.
