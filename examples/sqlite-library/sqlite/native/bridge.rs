use std::collections::BTreeMap;
use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::ptr;
use std::sync::{Arc, Mutex, OnceLock};

use super::native;
use super::scheduler::BlockingGuard;
use super::strconv::ValueError;
use super::sys::{ByteArrayError, ErrorOut};

unsafe extern "C" {
    fn sqlite3_open_v2(
        path: *const c_char,
        out: *mut *mut c_void,
        flags: c_int,
        vfs: *const c_char,
    ) -> c_int;
    fn sqlite3_close_v2(db: *mut c_void) -> c_int;
    fn sqlite3_errmsg(db: *mut c_void) -> *const c_char;
    fn sqlite3_errcode(db: *mut c_void) -> c_int;
    fn sqlite3_prepare_v2(
        db: *mut c_void,
        sql: *const c_char,
        length: c_int,
        out: *mut *mut c_void,
        tail: *mut *const c_char,
    ) -> c_int;
    fn sqlite3_finalize(stmt: *mut c_void) -> c_int;
    fn sqlite3_step(stmt: *mut c_void) -> c_int;
    fn sqlite3_reset(stmt: *mut c_void) -> c_int;
    fn sqlite3_clear_bindings(stmt: *mut c_void) -> c_int;
    fn sqlite3_bind_parameter_count(stmt: *mut c_void) -> c_int;
    fn sqlite3_bind_null(stmt: *mut c_void, index: c_int) -> c_int;
    fn sqlite3_bind_int64(stmt: *mut c_void, index: c_int, value: i64) -> c_int;
    fn sqlite3_bind_double(stmt: *mut c_void, index: c_int, value: f64) -> c_int;
    fn sqlite3_bind_text(
        stmt: *mut c_void,
        index: c_int,
        data: *const c_char,
        length: c_int,
        destructor: isize,
    ) -> c_int;
    fn sqlite3_bind_blob(
        stmt: *mut c_void,
        index: c_int,
        data: *const c_void,
        length: c_int,
        destructor: isize,
    ) -> c_int;
    fn sqlite3_column_count(stmt: *mut c_void) -> c_int;
    fn sqlite3_column_type(stmt: *mut c_void, column: c_int) -> c_int;
    fn sqlite3_column_int64(stmt: *mut c_void, column: c_int) -> i64;
    fn sqlite3_column_double(stmt: *mut c_void, column: c_int) -> f64;
    fn sqlite3_column_text(stmt: *mut c_void, column: c_int) -> *const u8;
    fn sqlite3_column_blob(stmt: *mut c_void, column: c_int) -> *const c_void;
    fn sqlite3_column_bytes(stmt: *mut c_void, column: c_int) -> c_int;
    fn sqlite3_busy_timeout(db: *mut c_void, milliseconds: c_int) -> c_int;
    fn sqlite3_exec(
        db: *mut c_void,
        sql: *const c_char,
        callback: *const c_void,
        arg: *mut c_void,
        error: *mut *mut c_char,
    ) -> c_int;
    fn sqlite3_set_authorizer(
        db: *mut c_void,
        callback: Option<
            unsafe extern "C" fn(
                *mut c_void,
                c_int,
                *const c_char,
                *const c_char,
                *const c_char,
                *const c_char,
            ) -> c_int,
        >,
        arg: *mut c_void,
    ) -> c_int;
}

struct Statement {
    pointer: *mut c_void,
    phase: i32,
}

struct Connection {
    pointer: *mut c_void,
    statements: BTreeMap<i64, Statement>,
    live: bool,
    valid: bool,
    transaction: bool,
}

// SQLite serialized mode and the connection mutex cover every native access.
unsafe impl Send for Connection {}

impl Drop for Connection {
    fn drop(&mut self) {
        // SAFETY: all statement leases have ended and this owns the connection.
        unsafe { sqlite3_close_v2(self.pointer) };
        native::resource_released();
    }
}

type Lease = Arc<Mutex<Connection>>;

#[derive(Default)]
struct Registry {
    next: i64,
    databases: BTreeMap<i64, Lease>,
    statements: BTreeMap<i64, Lease>,
}

fn registry() -> &'static Mutex<Registry> {
    static REGISTRY: OnceLock<Mutex<Registry>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(Registry::default()))
}

fn token(registry: &mut Registry) -> Result<i64, String> {
    registry.next = registry
        .next
        .checked_add(1)
        .ok_or("sqlite: token space exhausted")?;
    Ok(registry.next)
}

fn database(id: i64) -> Result<Lease, String> {
    registry()
        .lock()
        .map_err(|_| "sqlite: registry unavailable")?
        .databases
        .get(&id)
        .cloned()
        .ok_or_else(|| "sqlite: invalid or closed database".into())
}

fn statement(id: i64) -> Result<Lease, String> {
    registry()
        .lock()
        .map_err(|_| "sqlite: registry unavailable")?
        .statements
        .get(&id)
        .cloned()
        .ok_or_else(|| "sqlite: invalid or closed statement".into())
}

fn status(db: *mut c_void, code: i32) -> Result<(), String> {
    if code == 0 {
        return Ok(());
    }
    // SAFETY: callers hold a lease and lock for this live connection.
    let message = unsafe { CStr::from_ptr(sqlite3_errmsg(db)) }.to_string_lossy();
    Err(format!("sqlite ({code}): {message}"))
}

unsafe extern "C" fn authorize(
    _: *mut c_void,
    action: c_int,
    _: *const c_char,
    _: *const c_char,
    _: *const c_char,
    _: *const c_char,
) -> c_int {
    if action == 22 || action == 32 { 1 } else { 0 }
}

fn execute(connection: &Connection, sql: &CStr) -> Result<(), String> {
    // SAFETY: the connection is exclusively locked and sql is terminated.
    unsafe {
        sqlite3_set_authorizer(connection.pointer, None, ptr::null_mut());
        let code = sqlite3_exec(
            connection.pointer,
            sql.as_ptr(),
            ptr::null(),
            ptr::null_mut(),
            ptr::null_mut(),
        );
        sqlite3_set_authorizer(connection.pointer, Some(authorize), ptr::null_mut());
        status(connection.pointer, code)
    }
}

unsafe fn input<'a>(data: *const u8, length: i64, limit: i64) -> Result<&'a [u8], String> {
    if !(0..=limit).contains(&length) || (length > 0 && data.is_null()) {
        return Err("sqlite: input exceeds size limit".into());
    }
    if length == 0 {
        return Ok(&[]);
    }
    // SAFETY: the generated caller lends length readable bytes through this call.
    Ok(unsafe { std::slice::from_raw_parts(data, length as usize) })
}

fn open(path: &[u8]) -> Result<i64, String> {
    let path = CString::new(path).map_err(|_| "sqlite: path contains NUL")?;
    let mut pointer = ptr::null_mut();
    // SAFETY: path is terminated and pointer is writable; full mutex mode is requested.
    let code =
        unsafe { sqlite3_open_v2(path.as_ptr(), &mut pointer, 2 | 4 | 0x10000, ptr::null()) };
    if code != 0 {
        let error = if pointer.is_null() {
            Err(format!("sqlite ({code}): open failed"))
        } else {
            status(pointer, code)
        };
        if !pointer.is_null() {
            // SAFETY: SQLite returns an owned connection even on most open failures.
            unsafe { sqlite3_close_v2(pointer) };
        }
        return error.map(|()| 0);
    }
    native::resource_created();
    let connection = Connection {
        pointer,
        statements: BTreeMap::new(),
        live: true,
        valid: true,
        transaction: false,
    };
    // SAFETY: this newly opened connection is exclusively owned.
    unsafe { sqlite3_set_authorizer(pointer, Some(authorize), ptr::null_mut()) };
    let mut registry = registry()
        .lock()
        .map_err(|_| "sqlite: registry unavailable")?;
    let id = token(&mut registry)?;
    registry
        .databases
        .insert(id, Arc::new(Mutex::new(connection)));
    Ok(id)
}

fn prepare(id: i64, sql: &[u8]) -> Result<i64, String> {
    let sql = CString::new(sql).map_err(|_| "sqlite: SQL contains NUL")?;
    let lease = database(id)?;
    let mut connection = lease.lock().map_err(|_| "sqlite: connection unavailable")?;
    if !connection.live || !connection.valid {
        return Err("sqlite: closed database".into());
    }
    let mut pointer = ptr::null_mut();
    let mut tail = ptr::null();
    // SAFETY: SQL is terminated and connection is locked; output pointers are writable.
    let code = unsafe {
        sqlite3_prepare_v2(
            connection.pointer,
            sql.as_ptr(),
            -1,
            &mut pointer,
            &mut tail,
        )
    };
    let result = status(connection.pointer, code).and_then(|()| {
        if pointer.is_null() {
            return Err("sqlite: expected one nonempty statement".into());
        }
        // SAFETY: SQLite returns a tail inside the terminated SQL input.
        if !unsafe { CStr::from_ptr(tail) }
            .to_bytes()
            .iter()
            .all(u8::is_ascii_whitespace)
        {
            return Err("sqlite: expected exactly one statement".into());
        }
        Ok(())
    });
    if let Err(error) = result {
        // SAFETY: finalize accepts null or the owned partially prepared statement.
        unsafe { sqlite3_finalize(pointer) };
        return Err(error);
    }
    let mut registry = registry()
        .lock()
        .map_err(|_| "sqlite: registry unavailable")?;
    let id = match token(&mut registry) {
        Ok(id) => id,
        Err(error) => {
            // SAFETY: this is the sole owner of the prepared statement.
            unsafe { sqlite3_finalize(pointer) };
            return Err(error);
        }
    };
    connection
        .statements
        .insert(id, Statement { pointer, phase: 0 });
    registry.statements.insert(id, lease.clone());
    native::resource_created();
    Ok(id)
}

fn with_statement<T>(
    id: i64,
    action: impl FnOnce(&mut Connection, *mut c_void, i32) -> Result<T, String>,
) -> Result<T, String> {
    let lease = statement(id)?;
    let mut connection = lease.lock().map_err(|_| "sqlite: connection unavailable")?;
    if !connection.valid {
        return Err("sqlite: abandoned transaction".into());
    }
    let statement = connection
        .statements
        .get(&id)
        .ok_or("sqlite: closed statement")?;
    let (pointer, phase) = (statement.pointer, statement.phase);
    action(&mut connection, pointer, phase)
}

fn finish(id: i64, commit: bool) -> Result<(), String> {
    let lease = database(id)?;
    let mut connection = lease.lock().map_err(|_| "sqlite: connection unavailable")?;
    if !connection.transaction {
        return Err("sqlite: no active transaction".into());
    }
    if !connection.statements.is_empty() {
        return Err("sqlite: close statements before transaction completion".into());
    }
    execute(&connection, if commit { c"COMMIT" } else { c"ROLLBACK" })?;
    connection.transaction = false;
    Ok(())
}

fn drop_database(id: i64, abort: bool) {
    let lease = registry()
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .databases
        .remove(&id);
    if let Some(lease) = lease {
        let mut connection = lease.lock().unwrap_or_else(|error| error.into_inner());
        connection.live = false;
        if abort || connection.transaction {
            connection.valid = false;
            let _ = execute(&connection, c"ROLLBACK");
            connection.transaction = false;
        }
    }
}

fn finalize(id: i64) -> Result<(), String> {
    let lease = registry()
        .lock()
        .map_err(|_| "sqlite: registry unavailable")?
        .statements
        .remove(&id);
    let Some(lease) = lease else {
        return Ok(());
    };
    let mut connection = lease.lock().map_err(|_| "sqlite: connection unavailable")?;
    if let Some(statement) = connection.statements.remove(&id) {
        // SAFETY: removal transfers exclusive ownership of this statement.
        let code = unsafe { sqlite3_finalize(statement.pointer) };
        native::resource_released();
        status(connection.pointer, code)?;
    }
    Ok(())
}

fn column(pointer: *mut c_void, phase: i32, index: i64, kind: i64) -> Result<i32, String> {
    // SAFETY: the caller holds the statement's connection mutex and lease.
    let count = unsafe { sqlite3_column_count(pointer) } as i64;
    if phase != 1 || !(0..count).contains(&index) {
        return Err("sqlite: column requires a current row and valid index".into());
    }
    // SAFETY: the index is in range and the statement has a current row.
    if kind != -1 && unsafe { sqlite3_column_type(pointer, index as i32) } as i64 != kind {
        return Err("sqlite: column type mismatch".into());
    }
    Ok(index as i32)
}

macro_rules! value {
    ($name:ident($($arg:ident : $ty:ty),*) $body:block) => {
        /// # Safety
        /// out is writable and borrowed buffers remain readable throughout this call.
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $name(out: *mut ValueError, $($arg: $ty),*) {
            let _blocking = BlockingGuard::enter();
            fn operation($($arg: $ty),*) -> Result<i64, String> $body
            let result = operation($($arg),*);
            let output = match result { Ok(value) => ValueError::ok(value), Err(error) => ValueError::failed_text(&error) };
            // SAFETY: the generated caller supplies writable result storage.
            unsafe { out.write(output) };
        }
    }
}

macro_rules! error {
    ($name:ident($($arg:ident : $ty:ty),*) $body:block) => {
        /// # Safety
        /// out is writable and borrowed buffers remain readable throughout this call.
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $name(out: *mut ErrorOut, $($arg: $ty),*) {
            let _blocking = BlockingGuard::enter();
            fn operation($($arg: $ty),*) -> Result<(), String> $body
            let result = operation($($arg),*);
            let output = match result { Ok(()) => ErrorOut::ok(), Err(error) => ErrorOut::failed(&error) };
            // SAFETY: the generated caller supplies writable result storage.
            unsafe { out.write(output) };
        }
    }
}

value!(zore_native_sqlite_native_open(data: *const u8, length: i64) {
    // SAFETY: the generated caller lends its input buffer for the call.
    open(unsafe { input(data, length, 1048576) }?)
});
value!(zore_native_sqlite_native_prepare(id: i64, data: *const u8, length: i64) {
    // SAFETY: the generated caller lends its input buffer for the call.
    prepare(id, unsafe { input(data, length, 1048576) }?)
});
error!(zore_native_sqlite_native_busy(id: i64, milliseconds: i64) {
    if !(0..=60000).contains(&milliseconds) { return Err("sqlite: busy timeout must be 0..60000 milliseconds".into()); }
    let lease = database(id)?;
    let connection = lease.lock().map_err(|_| "sqlite: connection unavailable")?;
    if !connection.live { return Err("sqlite: closed database".into()); }
    // SAFETY: connection is exclusively locked and milliseconds is in range.
    status(connection.pointer, unsafe { sqlite3_busy_timeout(connection.pointer, milliseconds as i32) })
});
error!(zore_native_sqlite_native_begin(id: i64) {
    let lease = database(id)?;
    let mut connection = lease.lock().map_err(|_| "sqlite: connection unavailable")?;
    if !connection.live || connection.transaction || !connection.statements.is_empty() { return Err("sqlite: Begin requires an open database without statements or transaction".into()); }
    execute(&connection, c"BEGIN IMMEDIATE")?;
    connection.transaction = true;
    Ok(())
});
error!(zore_native_sqlite_native_finish(id: i64, commit: bool) { finish(id, commit) });

#[unsafe(no_mangle)]
pub extern "C" fn zore_native_sqlite_native_drop_database(id: i64, abort: bool) {
    let _blocking = BlockingGuard::enter();
    drop_database(id, abort);
}

error!(zore_native_sqlite_native_finalize(id: i64) { finalize(id) });
error!(zore_native_sqlite_native_reset(id: i64) {
    with_statement(id, |connection, pointer, _| {
        // SAFETY: this live statement is exclusively locked.
        let code = unsafe { sqlite3_reset(pointer) };
        // SAFETY: clearing bindings is valid after reset, even after a step error.
        let cleared = unsafe { sqlite3_clear_bindings(pointer) };
        connection.statements.get_mut(&id).unwrap().phase = 0;
        status(connection.pointer, code).and_then(|()| status(connection.pointer, cleared))
    })
});
error!(zore_native_sqlite_native_bind(id: i64, index: i64, kind: i64, integer: i64, real: f64, data: *const u8, length: i64) {
    // SAFETY: the generated caller lends this buffer through the call.
    let bytes = unsafe { input(data, length, 16777216) }?;
    with_statement(id, |connection, pointer, phase| {
        // SAFETY: this live statement is exclusively locked.
        let count = unsafe { sqlite3_bind_parameter_count(pointer) } as i64;
        if phase != 0 || !(1..=count).contains(&index) { return Err("sqlite: binding requires Reset and a valid parameter index".into()); }
        if kind == 2 && real.is_nan() { return Err("sqlite: NaN cannot be bound as a real value".into()); }
        // SAFETY: indexes are checked; TRANSIENT copies all borrowed bytes before returning.
        let code = unsafe { match kind {
            5 => sqlite3_bind_null(pointer, index as i32),
            1 => sqlite3_bind_int64(pointer, index as i32, integer),
            2 => sqlite3_bind_double(pointer, index as i32, real),
            3 => sqlite3_bind_text(pointer, index as i32, bytes.as_ptr().cast(), bytes.len() as i32, -1),
            4 => sqlite3_bind_blob(pointer, index as i32, bytes.as_ptr().cast(), bytes.len() as i32, -1),
            _ => return Err("sqlite: invalid binding type".into()),
        }};
        status(connection.pointer, code)
    })
});
value!(zore_native_sqlite_native_step(id: i64) {
    with_statement(id, |connection, pointer, phase| {
        if phase == 2 { return Err("sqlite: Step requires Reset after completion or failure".into()); }
        // SAFETY: this live statement is exclusively locked.
        let code = unsafe { sqlite3_step(pointer) };
        connection.statements.get_mut(&id).unwrap().phase = if code == 100 { 1 } else { 2 };
        match code { 100 => Ok(1), 101 => Ok(0), _ => status(connection.pointer, code).map(|()| 0) }
    })
});
value!(zore_native_sqlite_native_column(id: i64, index: i64, kind: i64) {
    with_statement(id, |_, pointer, phase| {
        if kind == 0 {
            // SAFETY: this live statement is exclusively locked.
            return Ok(unsafe { sqlite3_column_count(pointer) } as i64);
        }
        let index = column(pointer, phase, index, kind)?;
        // SAFETY: column validated the row, index and exact type.
        Ok(unsafe { if kind == -1 { sqlite3_column_type(pointer, index) as i64 } else { sqlite3_column_int64(pointer, index) } })
    })
});
value!(zore_native_sqlite_native_float(id: i64, index: i64) {
    with_statement(id, |_, pointer, phase| {
        let index = column(pointer, phase, index, 2)?;
        // SAFETY: column validated the row, index and real type.
        Ok(unsafe { sqlite3_column_double(pointer, index) }.to_bits() as i64)
    })
});

/// # Safety
/// out is writable result storage supplied by the generated caller.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_sqlite_native_bytes(
    out: *mut ByteArrayError,
    id: i64,
    index: i64,
    kind: i64,
) {
    let _blocking = BlockingGuard::enter();
    let result = with_statement(id, |connection, pointer, phase| {
        if kind != 3 && kind != 4 {
            return Err("sqlite: expected text or blob".into());
        }
        let index = column(pointer, phase, index, kind)?;
        // SAFETY: column validated the current row and exact type; the lock preserves storage.
        let (data, length) = unsafe {
            let data = if kind == 3 {
                sqlite3_column_text(pointer, index)
            } else {
                sqlite3_column_blob(pointer, index).cast()
            };
            (data, sqlite3_column_bytes(pointer, index))
        };
        // SAFETY: the connection remains locked while inspecting conversion failure.
        if data.is_null() && (length > 0 || unsafe { sqlite3_errcode(connection.pointer) } == 7) {
            return status(connection.pointer, 7).map(|()| Vec::new());
        }
        // SAFETY: SQLite lends length bytes until the next statement operation under this lock.
        let bytes = if length == 0 {
            &[]
        } else {
            unsafe { std::slice::from_raw_parts(data, length as usize) }
        };
        if kind == 3 {
            std::str::from_utf8(bytes).map_err(|_| "sqlite: text is not valid UTF-8")?;
        }
        Ok(bytes.to_vec())
    });
    // SAFETY: the generated caller supplies writable result storage.
    unsafe { out.write(ByteArrayError::from_bytes(result)) };
}
