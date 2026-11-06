# Photo board

A small web application that uses the HTTP helpers from §37.16 to §37.18 and
the secure tokens of §37.19 together: a visitor signs in with a display name,
uploads photos, and everyone sees them on one page.

| File | Responsibility |
| --- | --- |
| `main.ore` | Startup, server limits, and routes |
| `handlers.ore` | Sessions, form tokens, uploads, validation, and file serving |
| `board.ore` | Mutex-protected photos and sessions |
| `page.ore` | The HTML page, with every value escaped |

Run `zore run examples/photo-board/main.ore` from a folder you can write to,
then open <http://127.0.0.1:8080>. Photos are stored in
`photo-board-data/photos` under the current folder. The list of photos lives
in memory, so that folder is emptied at every start; files from an earlier
run are never served or overwritten.

Every form carries a token, so a client has to read the page first. With
`curl`, from another terminal:

```sh
curl -s -c jar http://127.0.0.1:8080/ | grep -o 'name="login" value="[^"]*"'
curl -i -b jar -c jar -d 'login=<the value>&name=Ada Lovelace' http://127.0.0.1:8080/name
curl -s -b jar http://127.0.0.1:8080/ | grep -o 'name="form" value="[^"]*"'
curl -i -b jar --form-string 'form=<the value>' --form-string 'title=My cat' -F photo=@cat.png http://127.0.0.1:8080/photos
curl -i http://127.0.0.1:8080/photos/1.png
```

The limits are named constants at the top of `handlers.ore` and
`board.ore`. What each part shows:

- **Sessions.** Signing in creates a session on the server: a random
  43-character token from `rand.Text`, a second random token for forms, and
  the name. Only the session token goes into the `board_session` cookie, set
  through `ResponseWriter.SetCookie` with `Path=/`, `HttpOnly`,
  `SameSite=Lax`, and a 30-day `Max-Age`. Every request looks the token up
  with `subtle.ConstantTimeEqual`, so a cookie someone invents or edits finds
  no session, and the name shown always comes from the server. Signing in
  again ends the previous session. Sessions live in memory: the browser
  forgets the cookie after 30 days, but the server keeps a session until the
  visitor signs out or the server restarts.
- **Session limits.** One client address may hold 5 sessions; a sixth
  sign-in from it ends its own oldest one. The board holds at most 1,000
  sessions, and when it is full a new sign-in gets `503` instead of pushing
  anyone else out, so nobody can sign other visitors out by signing in many
  times. `POST /name/forget` ends the session on the server and always
  deletes the cookie.
- **Form tokens.** Each form sends a token the server compares with
  `ConstantTimeEqual` before doing anything; a missing or wrong token gets
  `403`. Pages from the board hold the right token, but another website
  cannot read them, so it cannot post a working form in a visitor's name.
  Signed-in forms carry the session's form token. The sign-in form, which has
  no session yet, uses a random `board_login` cookie of one hour and the same
  value in a hidden field (a double-submit token), so another site cannot
  sign a visitor in under a name it chose. Pages are sent with
  `Cache-Control: no-store`, so the tokens are not cached.
- **Forms.** `POST /name` and `POST /name/forget` read URL-encoded forms with
  `Request.ReadForm` and check the name before creating a session.
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

This is still an example, not a full login system: anyone can sign in under
any name, there are no passwords, sessions vanish on restart, and many
clients behind one proxy share one address and its 5 sessions. Behind HTTPS,
also set `Secure` on both cookies and rename them with the `__Host-` prefix,
so that a site on a sibling subdomain cannot plant a `board_login` cookie and
sign a visitor in under a name it chose. Photos are public: anyone who knows
a photo's URL can load it. Uploads without a session get `403`.

Press Ctrl-C (or send SIGTERM on Unix) to drain connections and stop.
`http.ListenAndServeWithConfig` handles the signals and graceful shutdown.
