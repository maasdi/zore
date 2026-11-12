#![cfg(windows)]

use std::collections::HashMap;
use std::ffi::{OsStr, c_void};
use std::fs::File;
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle, RawHandle};
use std::path::{Component, Path, Prefix};

const SYNCHRONIZE: u32 = 0x100000;
const READ_CONTROL: u32 = 0x20000;
const DELETE: u32 = 0x10000;
const READ_ATTRIBUTES: u32 = 0x80;
const TRAVERSE: u32 = 0x20;
const ADD_DIRECTORY: u32 = 4;
const ALL_ACCESS: u32 = 0x1f01ff;
const DIRECTORY: u32 = 1;
const NON_DIRECTORY: u32 = 0x40;
const SYNCHRONOUS: u32 = 0x20;
const OPEN_REPARSE: u32 = 0x200000;
const OPEN: u32 = 1;
const CREATE: u32 = 2;
const REPARSE_ATTRIBUTE: u32 = 0x400;
const DIRECTORY_ATTRIBUTE: u32 = 0x10;
const PENDING: i32 = 0x103;

type Handle = *mut c_void;

#[repr(C)]
struct UnicodeString {
    length: u16,
    capacity: u16,
    data: *mut u16,
}
#[repr(C)]
struct ObjectAttributes {
    length: u32,
    root: Handle,
    name: *mut UnicodeString,
    attributes: u32,
    security: *mut c_void,
    qos: *mut c_void,
}
#[repr(C)]
struct IoStatus {
    status: isize,
    information: usize,
}
#[repr(C)]
struct AttributeTag {
    attributes: u32,
    tag: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct FileId {
    volume: u64,
    id: [u8; 16],
}
#[repr(C)]
struct AclSize {
    count: u32,
    used: u32,
    free: u32,
}
#[repr(C)]
struct AceHeader {
    kind: u8,
    flags: u8,
    size: u16,
}

#[link(name = "ntdll")]
unsafe extern "system" {
    fn NtCreateFile(
        handle: *mut Handle,
        access: u32,
        attributes: *const ObjectAttributes,
        io: *mut IoStatus,
        size: *const i64,
        file_attributes: u32,
        share: u32,
        disposition: u32,
        options: u32,
        ea: *const c_void,
        ea_size: u32,
    ) -> i32;
    fn NtSetInformationFile(
        handle: Handle,
        io: *mut IoStatus,
        data: *const c_void,
        length: u32,
        class: u32,
    ) -> i32;
    fn RtlNtStatusToDosError(status: i32) -> u32;
}
#[link(name = "kernel32")]
unsafe extern "system" {
    fn CreateFileW(
        name: *const u16,
        access: u32,
        share: u32,
        security: *const c_void,
        disposition: u32,
        flags: u32,
        template: Handle,
    ) -> Handle;
    fn GetFileInformationByHandleEx(
        handle: Handle,
        class: u32,
        data: *mut c_void,
        length: u32,
    ) -> i32;
    fn SetFileInformationByHandle(
        handle: Handle,
        class: u32,
        data: *const c_void,
        length: u32,
    ) -> i32;
    fn GetCurrentProcess() -> Handle;
    fn GetCurrentThread() -> Handle;
    fn WaitForSingleObject(handle: Handle, timeout: u32) -> u32;
    fn LocalFree(memory: *mut c_void) -> *mut c_void;
}
#[link(name = "advapi32")]
unsafe extern "system" {
    fn OpenProcessToken(process: Handle, access: u32, token: *mut Handle) -> i32;
    fn OpenThreadToken(thread: Handle, access: u32, as_self: i32, token: *mut Handle) -> i32;
    fn GetTokenInformation(
        token: Handle,
        class: u32,
        data: *mut c_void,
        size: u32,
        used: *mut u32,
    ) -> i32;
    fn GetLengthSid(sid: *const c_void) -> u32;
    fn EqualSid(first: *const c_void, second: *const c_void) -> i32;
    fn IsWellKnownSid(sid: *const c_void, kind: u32) -> i32;
    fn ConvertSidToStringSidW(sid: *const c_void, text: *mut *mut u16) -> i32;
    fn ConvertStringSecurityDescriptorToSecurityDescriptorW(
        text: *const u16,
        revision: u32,
        descriptor: *mut *mut c_void,
        size: *mut u32,
    ) -> i32;
    fn GetSecurityInfo(
        handle: Handle,
        kind: u32,
        information: u32,
        owner: *mut *mut c_void,
        group: *mut *mut c_void,
        dacl: *mut *mut c_void,
        sacl: *mut *mut c_void,
        descriptor: *mut *mut c_void,
    ) -> u32;
    fn GetSecurityDescriptorControl(
        descriptor: *const c_void,
        control: *mut u16,
        revision: *mut u32,
    ) -> i32;
    fn GetAclInformation(acl: *const c_void, data: *mut c_void, size: u32, class: u32) -> i32;
    fn GetAce(acl: *const c_void, index: u32, ace: *mut *mut c_void) -> i32;
}

fn invalid(message: &str) -> io::Error {
    io::Error::other(message)
}
fn words(bytes: usize) -> Vec<usize> {
    vec![0; bytes.div_ceil(size_of::<usize>())]
}
fn wide(value: &OsStr) -> Vec<u16> {
    value.encode_wide().collect()
}
fn component(value: &[u16]) -> io::Result<()> {
    if value.is_empty()
        || value == [46]
        || value == [46, 46]
        || value.iter().any(|c| matches!(c, 0 | 47 | 58 | 92))
    {
        return Err(invalid("invalid path component"));
    }
    if value.len() > 32767 {
        return Err(invalid("path component too long"));
    }
    Ok(())
}
fn nt_result(mut status: i32, io: &IoStatus, handle: Handle) -> io::Result<()> {
    if status == PENDING {
        // SAFETY: synchronous file handles and the status storage stay live until completion.
        if unsafe { WaitForSingleObject(handle, u32::MAX) } != 0 {
            return Err(io::Error::last_os_error());
        }
        status = io.status as i32;
    }
    if status < 0 {
        // SAFETY: this conversion accepts any NT status value.
        return Err(io::Error::from_raw_os_error(
            unsafe { RtlNtStatusToDosError(status) } as i32,
        ));
    }
    Ok(())
}
fn information<T>(file: &File, class: u32, value: &mut T) -> io::Result<()> {
    // SAFETY: the class and matching initialized output type are chosen only by this module.
    if unsafe {
        GetFileInformationByHandleEx(
            file.as_raw_handle(),
            class,
            std::ptr::from_mut(value).cast(),
            size_of::<T>() as u32,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}
fn identity(file: &File, directory: bool) -> io::Result<FileId> {
    let mut tag = AttributeTag {
        attributes: 0,
        tag: 0,
    };
    information(file, 9, &mut tag)?;
    if tag.attributes & REPARSE_ATTRIBUTE != 0
        || (tag.attributes & DIRECTORY_ATTRIBUTE != 0) != directory
    {
        return Err(invalid(
            "reparse points and unexpected node types are forbidden",
        ));
    }
    let mut id = FileId {
        volume: 0,
        id: [0; 16],
    };
    information(file, 18, &mut id)?;
    Ok(id)
}
fn open(
    parent: &File,
    name: &[u16],
    directory: bool,
    create: bool,
    access: u32,
    security: Option<&Identity>,
) -> io::Result<File> {
    component(name)?;
    let mut name = name.to_vec();
    let length = (name.len() * 2) as u16;
    let mut unicode = UnicodeString {
        length,
        capacity: length,
        data: name.as_mut_ptr(),
    };
    let attributes = ObjectAttributes {
        length: size_of::<ObjectAttributes>() as u32,
        root: parent.as_raw_handle(),
        name: &mut unicode,
        attributes: 0x40 | 0x1000,
        security: security.map_or(std::ptr::null_mut(), |s| {
            s.descriptor.as_ptr().cast_mut().cast()
        }),
        qos: std::ptr::null_mut(),
    };
    let mut handle = std::ptr::null_mut();
    let mut io = IoStatus {
        status: 0,
        information: 0,
    };
    // SAFETY: all descriptor/name/status storage remains valid through synchronous completion.
    let status = unsafe {
        NtCreateFile(
            &mut handle,
            access | SYNCHRONIZE,
            &attributes,
            &mut io,
            std::ptr::null(),
            if directory { DIRECTORY_ATTRIBUTE } else { 0x80 },
            7,
            if create { CREATE } else { OPEN },
            SYNCHRONOUS | OPEN_REPARSE | if directory { DIRECTORY } else { NON_DIRECTORY },
            std::ptr::null(),
            0,
        )
    };
    nt_result(status, &io, handle)?;
    if handle.is_null() || handle == -1isize as Handle {
        return Err(invalid("invalid native file handle"));
    }
    // SAFETY: NtCreateFile returned a new owned file handle.
    let file = unsafe { File::from_raw_handle(handle) };
    if create && io.information != 2 {
        return Err(invalid("filesystem did not create an exclusive node"));
    }
    Ok(file)
}
struct LocalMemory(*mut c_void);
impl Drop for LocalMemory {
    fn drop(&mut self) {
        // SAFETY: each non-null allocation originates from a LocalAlloc-backed Windows API.
        unsafe { LocalFree(self.0) };
    }
}
struct Identity {
    sid: Vec<usize>,
    descriptor: Vec<usize>,
}
impl Identity {
    fn current() -> io::Result<Self> {
        let mut token = std::ptr::null_mut();
        // SAFETY: token is writable; the pseudo-handle belongs to this thread.
        if unsafe { OpenThreadToken(GetCurrentThread(), 8, 1, &mut token) } == 0 {
            if io::Error::last_os_error().raw_os_error() != Some(1008) {
                return Err(io::Error::last_os_error());
            }
            // SAFETY: token is writable; the pseudo-handle belongs to this process.
            if unsafe { OpenProcessToken(GetCurrentProcess(), 8, &mut token) } == 0 {
                return Err(io::Error::last_os_error());
            }
        }
        // SAFETY: successful token opening returned a fresh owned kernel handle.
        let token = unsafe { OwnedHandle::from_raw_handle(token) };
        let mut length = 0;
        // SAFETY: a zero-sized query writes only the required length.
        unsafe {
            GetTokenInformation(
                token.as_raw_handle(),
                1,
                std::ptr::null_mut(),
                0,
                &mut length,
            )
        };
        if length == 0 || length > 65536 {
            return Err(invalid("invalid effective token information size"));
        }
        let mut storage = words(length as usize);
        // SAFETY: storage is aligned and has at least length writable bytes.
        if unsafe {
            GetTokenInformation(
                token.as_raw_handle(),
                1,
                storage.as_mut_ptr().cast(),
                length,
                &mut length,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: TOKEN_USER starts with SID_AND_ATTRIBUTES, whose first field is a SID pointer.
        let sid = unsafe { *storage.as_ptr().cast::<*const c_void>() };
        // SAFETY: the token query supplied a valid SID inside the live buffer.
        let sid_length = unsafe { GetLengthSid(sid) } as usize;
        if !(8..=68).contains(&sid_length) {
            return Err(invalid("invalid effective user SID"));
        }
        let mut owned_sid = words(sid_length);
        // SAFETY: the returned SID spans sid_length bytes and the destination is large enough.
        unsafe {
            std::ptr::copy_nonoverlapping(
                sid.cast::<u8>(),
                owned_sid.as_mut_ptr().cast(),
                sid_length,
            )
        };
        let mut text = std::ptr::null_mut();
        // SAFETY: sid is valid and text is writable for the allocated string pointer.
        if unsafe { ConvertSidToStringSidW(sid, &mut text) } == 0 {
            return Err(io::Error::last_os_error());
        }
        let text_memory = LocalMemory(text.cast());
        let mut count = 0;
        // SAFETY: Windows SID strings are NUL-terminated and bounded by the SID representation.
        while count < 184 && unsafe { *text.add(count) } != 0 {
            count += 1;
        }
        if count == 184 {
            return Err(invalid("invalid SID text"));
        }
        // SAFETY: the loop established the readable string length.
        let sid_text = String::from_utf16(unsafe { std::slice::from_raw_parts(text, count) })
            .map_err(|_| invalid("invalid SID text"))?;
        drop(text_memory);
        let sddl = format!("O:{sid_text}D:P(A;OICI;FA;;;{sid_text})(A;OICI;FA;;;SY)");
        let mut sddl: Vec<u16> = sddl.encode_utf16().collect();
        sddl.push(0);
        let mut descriptor = std::ptr::null_mut();
        let mut size = 0;
        // SAFETY: the input is NUL-terminated and both outputs are writable.
        if unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl.as_ptr(),
                1,
                &mut descriptor,
                &mut size,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        let memory = LocalMemory(descriptor);
        if size == 0 || size > 65536 {
            return Err(invalid("invalid private security descriptor"));
        }
        let mut owned_descriptor = words(size as usize);
        // SAFETY: Windows returned size initialized bytes; relative descriptor offsets survive copying.
        unsafe {
            std::ptr::copy_nonoverlapping(
                descriptor.cast::<u8>(),
                owned_descriptor.as_mut_ptr().cast(),
                size as usize,
            )
        };
        drop(memory);
        Ok(Self {
            sid: owned_sid,
            descriptor: owned_descriptor,
        })
    }
    fn verify(&self, file: &File) -> io::Result<()> {
        let mut owner = std::ptr::null_mut();
        let mut dacl = std::ptr::null_mut();
        let mut descriptor = std::ptr::null_mut();
        // SAFETY: the live file grants READ_CONTROL and all requested output pointers are writable.
        let code = unsafe {
            GetSecurityInfo(
                file.as_raw_handle(),
                1,
                1 | 4,
                &mut owner,
                std::ptr::null_mut(),
                &mut dacl,
                std::ptr::null_mut(),
                &mut descriptor,
            )
        };
        if code != 0 {
            return Err(io::Error::from_raw_os_error(code as i32));
        }
        let _memory = LocalMemory(descriptor);
        let mut control = 0;
        let mut revision = 0;
        // SAFETY: GetSecurityInfo supplied a valid descriptor and owner SID.
        if unsafe { GetSecurityDescriptorControl(descriptor, &mut control, &mut revision) } == 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: both SIDs originate from successful native security queries.
        if owner.is_null()
            || dacl.is_null()
            || control & 0x1000 == 0
            || unsafe { EqualSid(owner, self.sid.as_ptr().cast()) } == 0
        {
            return Err(invalid(
                "filesystem did not establish private ownership and a protected DACL",
            ));
        }
        let mut info = AclSize {
            count: 0,
            used: 0,
            free: 0,
        };
        // SAFETY: the ACL is valid and info matches ACL_SIZE_INFORMATION.
        if unsafe {
            GetAclInformation(
                dacl,
                std::ptr::from_mut(&mut info).cast(),
                size_of::<AclSize>() as u32,
                2,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        if info.count == 0 || info.count > 2 {
            return Err(invalid("unexpected private DACL entries"));
        }
        let mut user = false;
        let mut system = false;
        for index in 0..info.count {
            let mut ace = std::ptr::null_mut();
            // SAFETY: index is below the queried ACE count and ace is writable.
            if unsafe { GetAce(dacl, index, &mut ace) } == 0 {
                return Err(io::Error::last_os_error());
            }
            // SAFETY: GetAce returned a valid ACE header and access mask.
            let header = unsafe { &*ace.cast::<AceHeader>() };
            if header.kind != 0 || header.flags & !3 != 0 || header.size < 16 {
                return Err(invalid("unexpected private DACL entry"));
            }
            // SAFETY: the checked ACCESS_ALLOWED_ACE header includes a mask and SID prefix.
            let mask = unsafe { *ace.cast::<u8>().add(4).cast::<u32>() };
            // SAFETY: a standard allowed ACE stores its SID at byte eight.
            let sid = unsafe { ace.cast::<u8>().add(8).cast::<c_void>() };
            // SAFETY: the native ACL contains a validated SID.
            let size = unsafe { GetLengthSid(sid) };
            if size > u32::from(header.size) - 8 || mask != ALL_ACCESS {
                return Err(invalid("unexpected private DACL rights"));
            }
            // SAFETY: all compared SIDs have validated native representations.
            let (is_user, is_system) = unsafe {
                (
                    EqualSid(sid, self.sid.as_ptr().cast()) != 0,
                    IsWellKnownSid(sid, 22) != 0,
                )
            };
            if !is_user && !is_system {
                return Err(invalid("private DACL grants another identity"));
            }
            user |= is_user;
            system |= is_system;
        }
        if !user || !system {
            return Err(invalid("private DACL is incomplete"));
        }
        Ok(())
    }
}

fn root_handle(path: &OsStr) -> io::Result<File> {
    let mut name = wide(path);
    name.push(0);
    // SAFETY: name is NUL-terminated; the remaining pointers intentionally select default security.
    let handle = unsafe {
        CreateFileW(
            name.as_ptr(),
            READ_ATTRIBUTES | TRAVERSE | SYNCHRONIZE,
            7,
            std::ptr::null(),
            3,
            0x02000000 | OPEN_REPARSE,
            std::ptr::null_mut(),
        )
    };
    if handle == -1isize as RawHandle {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: CreateFileW returned a new owned file handle.
    let file = unsafe { File::from_raw_handle(handle) };
    identity(&file, true)?;
    Ok(file)
}
fn parent_path(destination: &str) -> io::Result<(File, Vec<u16>)> {
    let normalized = destination.replace('/', "\\");
    let destination = normalized.as_str();
    let basename = destination
        .rsplit(['/', '\\'])
        .next()
        .ok_or_else(|| invalid("invalid destination"))?;
    component(&basename.encode_utf16().collect::<Vec<_>>())?;
    let path = Path::new(destination);
    if path.components().any(|p| matches!(p, Component::ParentDir)) {
        return Err(invalid("parent traversal is forbidden"));
    }
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        if matches!(
            path.components().next(),
            Some(Component::Prefix(_) | Component::RootDir)
        ) {
            return Err(invalid(
                "drive-relative and root-relative destinations are forbidden",
            ));
        }
        std::env::current_dir()?.join(path)
    };
    let mut components = absolute.components();
    let root = match components.next() {
        Some(Component::Prefix(prefix)) => match prefix.kind() {
            Prefix::Disk(letter) | Prefix::VerbatimDisk(letter) if letter.is_ascii_alphabetic() => {
                format!("{}:\\", char::from(letter))
            }
            Prefix::UNC(server, share) | Prefix::VerbatimUNC(server, share) => {
                component(&wide(server))?;
                component(&wide(share))?;
                format!(
                    "\\\\{}\\{}\\",
                    server
                        .to_str()
                        .ok_or_else(|| invalid("invalid server name"))?,
                    share
                        .to_str()
                        .ok_or_else(|| invalid("invalid share name"))?
                )
            }
            _ => return Err(invalid("device namespaces are forbidden")),
        },
        _ => return Err(invalid("absolute Windows destination required")),
    };
    if !matches!(components.next(), Some(Component::RootDir)) {
        return Err(invalid("invalid Windows root"));
    }
    let mut names = Vec::new();
    for part in components {
        match part {
            Component::CurDir => (),
            Component::Normal(name) => {
                let name = wide(name);
                component(&name)?;
                names.push(name);
            }
            _ => return Err(invalid("invalid destination component")),
        }
    }
    let destination = names
        .pop()
        .ok_or_else(|| invalid("invalid destination basename"))?;
    let mut parent = root_handle(OsStr::new(&root))?;
    for (index, name) in names.iter().enumerate() {
        let access = READ_ATTRIBUTES
            | TRAVERSE
            | if index + 1 == names.len() {
                ADD_DIRECTORY
            } else {
                0
            };
        parent = open(&parent, name, true, false, access, None)?;
        identity(&parent, true)?;
    }
    if names.is_empty() {
        let mut root_name = wide(OsStr::new(&root));
        root_name.push(0);
        // SAFETY: root_name is NUL-terminated and no security/template pointer is supplied.
        let handle = unsafe {
            CreateFileW(
                root_name.as_ptr(),
                READ_ATTRIBUTES | TRAVERSE | SYNCHRONIZE | ADD_DIRECTORY,
                7,
                std::ptr::null(),
                3,
                0x02000000 | OPEN_REPARSE,
                std::ptr::null_mut(),
            )
        };
        if handle == -1isize as RawHandle {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: CreateFileW returned a fresh owned directory handle.
        parent = unsafe { File::from_raw_handle(handle) };
        identity(&parent, true)?;
    }
    match open(&parent, &destination, false, false, READ_ATTRIBUTES, None) {
        Ok(_) => Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "destination exists",
        )),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok((parent, destination)),
        Err(error) => Err(error),
    }
}
fn delete(file: &File) -> io::Result<()> {
    let flags = 1u32 | 2 | 16;
    // SAFETY: the handle has DELETE access and flags matches FILE_DISPOSITION_INFO_EX.
    if unsafe {
        SetFileInformationByHandle(
            file.as_raw_handle(),
            21,
            std::ptr::from_ref(&flags).cast(),
            4,
        )
    } != 0
    {
        return Ok(());
    }
    let error = io::Error::last_os_error();
    if !matches!(error.raw_os_error(), Some(1 | 50 | 87)) {
        return Err(error);
    }
    let delete = 1u8;
    // SAFETY: the fallback marks this same handle for deletion without resolving a path.
    if unsafe {
        SetFileInformationByHandle(
            file.as_raw_handle(),
            4,
            std::ptr::from_ref(&delete).cast(),
            1,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}
fn rename(file: &File, parent: &File, name: &[u16]) -> io::Result<()> {
    place(file, parent, name, 10)
}
fn hard_link(file: &File, parent: &File, name: &[u16]) -> io::Result<()> {
    place(file, parent, name, 11)
}
fn place(file: &File, parent: &File, name: &[u16], class: u32) -> io::Result<()> {
    let size = 20 + name.len() * 2;
    let mut data = words(size);
    let bytes = data.as_mut_ptr().cast::<u8>();
    // SAFETY: the aligned buffer has the 64-bit FILE_RENAME/LINK_INFORMATION header and complete name.
    unsafe {
        bytes.add(8).cast::<Handle>().write(parent.as_raw_handle());
        bytes.add(16).cast::<u32>().write((name.len() * 2) as u32);
        std::ptr::copy_nonoverlapping(name.as_ptr(), bytes.add(20).cast(), name.len());
    }
    let mut io = IoStatus {
        status: 0,
        information: 0,
    };
    // SAFETY: ReplaceIfExists remains false and all storage/handles remain live through completion.
    let status = unsafe {
        NtSetInformationFile(
            file.as_raw_handle(),
            &mut io,
            data.as_ptr().cast(),
            size as u32,
            class,
        )
    };
    nt_result(status, &io, file.as_raw_handle())
}

struct Node {
    path: String,
    directory: bool,
    id: FileId,
}
pub(super) struct Tree {
    parent: File,
    destination: Vec<u16>,
    envelope: Option<File>,
    root: Option<File>,
    security: Identity,
    nodes: Vec<Node>,
    directories: HashMap<String, FileId>,
    identities: HashMap<FileId, String>,
    pending: Vec<File>,
    limit: usize,
    active: Option<File>,
    published: bool,
    cleaned: bool,
}
impl Tree {
    pub(super) fn create(destination: &str, limit: usize) -> io::Result<Self> {
        if !cfg!(target_arch = "x86_64") {
            return Err(invalid(
                "confined extraction is unsupported on this architecture",
            ));
        }
        let (parent, destination) = parent_path(destination)?;
        Self::from_parent(parent, destination, limit)
    }
    fn from_parent(parent: File, destination: Vec<u16>, limit: usize) -> io::Result<Self> {
        let security = Identity::current()?;
        let mut envelope = None;
        for _ in 0..100 {
            let random = super::crypto::random_bytes(16).map_err(|e| invalid(&e))?;
            let suffix: String = random.iter().map(|b| format!("{b:02x}")).collect();
            let name: Vec<u16> = format!(".zore-stage-{suffix}").encode_utf16().collect();
            match open(&parent, &name, true, true, ALL_ACCESS, Some(&security)) {
                Ok(file) => {
                    envelope = Some(file);
                    break;
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => (),
                Err(error) => return Err(error),
            }
        }
        let envelope = envelope.ok_or_else(|| invalid("no staging name available"))?;
        let mut tree = Self {
            parent,
            destination,
            envelope: Some(envelope),
            root: None,
            security,
            nodes: Vec::new(),
            directories: HashMap::new(),
            identities: HashMap::new(),
            pending: Vec::new(),
            limit,
            active: None,
            published: false,
            cleaned: false,
        };
        let initialized = (|| {
            let envelope = tree.envelope.as_ref().expect("created above");
            identity(envelope, true)?;
            tree.security.verify(envelope)?;
            tree.root = Some(open(
                envelope,
                &"tree".encode_utf16().collect::<Vec<_>>(),
                true,
                true,
                ALL_ACCESS,
                Some(&tree.security),
            )?);
            let root = tree.root.as_ref().expect("created above");
            identity(root, true)?;
            tree.security.verify(root)
        })();
        if let Err(original) = initialized {
            return match tree.abort() {
                Ok(()) => Err(original),
                Err(cleanup) => Err(invalid(&format!("{original}; cleanup failed: {cleanup}"))),
            };
        }
        Ok(tree)
    }
    fn parts(path: &str) -> io::Result<Vec<&str>> {
        let parts: Vec<_> = path.split('/').collect();
        if path.len() > 1024 || parts.len() > 64 {
            return Err(invalid("local path limit exceeded"));
        }
        for part in &parts {
            component(&part.encode_utf16().collect::<Vec<_>>())?;
        }
        Ok(parts)
    }
    fn register(&mut self, file: File, path: &str, directory: bool) -> io::Result<File> {
        let verified = (|| {
            let id = identity(&file, directory)?;
            self.security.verify(&file)?;
            Ok::<_, io::Error>(id)
        })();
        let id = match verified {
            Ok(id) => id,
            Err(error) => {
                self.pending.push(file);
                return Err(error);
            }
        };
        if self.identities.contains_key(&id) {
            self.pending.push(file);
            return Err(invalid("filesystem name alias"));
        }
        self.identities.insert(id, path.into());
        self.nodes.push(Node {
            path: path.into(),
            directory,
            id,
        });
        if directory {
            self.directories.insert(path.into(), id);
        }
        Ok(file)
    }
    fn walk(&mut self, parts: &[&str], create: bool) -> io::Result<File> {
        let mut current = self
            .root
            .as_ref()
            .ok_or_else(|| invalid("staged tree is closed"))?
            .try_clone()?;
        let mut path = String::new();
        for part in parts {
            if !path.is_empty() {
                path.push('/');
            }
            path.push_str(part);
            let name: Vec<_> = part.encode_utf16().collect();
            if let Some(expected) = self.directories.get(&path) {
                let next = open(&current, &name, true, false, ALL_ACCESS, None)?;
                if identity(&next, true)? != *expected {
                    return Err(invalid("staging directory identity changed"));
                }
                current = next;
            } else {
                if !create {
                    return Err(invalid("untracked staging directory"));
                }
                if self.nodes.len() == self.limit {
                    return Err(invalid("node limit exceeded"));
                }
                let next = open(
                    &current,
                    &name,
                    true,
                    true,
                    ALL_ACCESS,
                    Some(&self.security),
                )?;
                current = self.register(next, &path, true)?;
            }
        }
        Ok(current)
    }
    pub(super) fn mkdir(&mut self, path: &str) -> io::Result<()> {
        if self.active.is_some() {
            return Err(invalid("an entry file is still open"));
        }
        self.walk(&Self::parts(path)?, true).map(drop)
    }
    pub(super) fn create_file(&mut self, path: &str, _executable: bool) -> io::Result<()> {
        if self.active.is_some() {
            return Err(invalid("an entry file is still open"));
        }
        let parts = Self::parts(path)?;
        let parent = self.walk(&parts[..parts.len() - 1], true)?;
        if self.nodes.len() == self.limit {
            return Err(invalid("node limit exceeded"));
        }
        let file = open(
            &parent,
            &parts[parts.len() - 1].encode_utf16().collect::<Vec<_>>(),
            false,
            true,
            ALL_ACCESS,
            Some(&self.security),
        )?;
        self.active = Some(self.register(file, path, false)?);
        Ok(())
    }
    pub(super) fn symlink(&mut self, _path: &str, _target: &str) -> io::Result<()> {
        Err(invalid(
            "symbolic links are unsupported in confined Windows extraction",
        ))
    }
    pub(super) fn hard_link(&mut self, path: &str, existing: &str) -> io::Result<()> {
        if self.active.is_some() {
            return Err(invalid("an entry file is still open"));
        }
        let expected = self
            .nodes
            .iter()
            .find(|node| node.path == existing && !node.directory)
            .map(|node| node.id)
            .ok_or_else(|| invalid("hard link target is not a staged file"))?;
        let old_parts = Self::parts(existing)?;
        let old_parent = self.walk(&old_parts[..old_parts.len() - 1], false)?;
        let target = open(
            &old_parent,
            &old_parts[old_parts.len() - 1]
                .encode_utf16()
                .collect::<Vec<_>>(),
            false,
            false,
            READ_ATTRIBUTES | 0x100,
            None,
        )?;
        if identity(&target, false)? != expected {
            return Err(invalid("staging node identity changed"));
        }
        let parts = Self::parts(path)?;
        let parent = self.walk(&parts[..parts.len() - 1], true)?;
        if self.nodes.len() == self.limit {
            return Err(invalid("node limit exceeded"));
        }
        let name: Vec<u16> = parts[parts.len() - 1].encode_utf16().collect();
        component(&name)?;
        hard_link(&target, &parent, &name)?;
        self.nodes.push(Node {
            path: path.into(),
            directory: false,
            id: expected,
        });
        Ok(())
    }
    pub(super) fn file(&mut self) -> io::Result<&mut File> {
        self.active
            .as_mut()
            .ok_or_else(|| invalid("no active file"))
    }
    pub(super) fn finish_file(&mut self) -> io::Result<()> {
        let file = self
            .active
            .take()
            .ok_or_else(|| invalid("no active file"))?;
        let synced = file.sync_all();
        let closed = super::os::close_reporting(file);
        synced.and(closed)
    }
    pub(super) fn commit(&mut self) -> io::Result<()> {
        if self.active.is_some() || !self.pending.is_empty() {
            return Err(invalid("an entry file is still open or unverified"));
        }
        rename(
            self.root
                .as_ref()
                .ok_or_else(|| invalid("staged tree is closed"))?,
            &self.parent,
            &self.destination,
        )?;
        self.published = true;
        self.nodes.clear();
        self.directories.clear();
        self.identities.clear();
        drop(self.root.take());
        self.abort().map_err(|e| {
            invalid(&format!(
                "tree published; staging envelope cleanup failed: {e}"
            ))
        })
    }
    pub(super) fn abort(&mut self) -> io::Result<()> {
        if self.cleaned {
            return Ok(());
        }
        let close_error = self
            .active
            .take()
            .and_then(|f| super::os::close_reporting(f).err());
        while let Some(file) = self.pending.last() {
            delete(file)?;
            drop(self.pending.pop());
        }
        while let Some(node) = self.nodes.last() {
            let path = node.path.clone();
            let directory = node.directory;
            let expected = node.id;
            let parts = Self::parts(&path)?;
            let parent = self.walk(&parts[..parts.len() - 1], false)?;
            let file = match open(
                &parent,
                &parts[parts.len() - 1].encode_utf16().collect::<Vec<_>>(),
                directory,
                false,
                DELETE | READ_ATTRIBUTES | READ_CONTROL,
                None,
            ) {
                Ok(file) => Some(file),
                Err(e) if e.kind() == io::ErrorKind::NotFound => None,
                Err(e) => return Err(e),
            };
            if let Some(file) = file {
                if identity(&file, directory)? != expected {
                    return Err(invalid("staging node identity changed"));
                }
                delete(&file)?;
                super::os::close_reporting(file)?;
            }
            self.nodes.pop();
            self.directories.remove(&path);
            self.identities.remove(&expected);
        }
        if let Some(root) = self.root.as_ref()
            && !self.published
        {
            delete(root)?;
        }
        drop(self.root.take());
        if let Some(envelope) = self.envelope.as_ref() {
            delete(envelope)?;
        }
        drop(self.envelope.take());
        self.cleaned = true;
        match close_error {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }
}
impl Drop for Tree {
    fn drop(&mut self) {
        let _ = self.abort();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::path::PathBuf;
    use std::process::Command;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetShortPathNameW(long: *const u16, short: *mut u16, size: u32) -> u32;
    }
    #[link(name = "advapi32")]
    unsafe extern "system" {
        fn SetFileSecurityW(name: *const u16, information: u32, descriptor: *const c_void) -> i32;
    }

    struct Sandbox(PathBuf);
    impl Sandbox {
        fn new() -> Self {
            let suffix: String = super::super::crypto::random_bytes(16)
                .unwrap()
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect();
            let root = std::env::temp_dir()
                .canonicalize()
                .unwrap()
                .join(format!("zore-windows-tree-{suffix}"));
            std::fs::create_dir(&root).unwrap();
            Self(root)
        }
        fn destination(&self) -> String {
            self.0.join("result").to_str().unwrap().into()
        }
        fn children(&self) -> usize {
            std::fs::read_dir(&self.0).unwrap().count()
        }
        fn staging(&self) -> PathBuf {
            std::fs::read_dir(&self.0)
                .unwrap()
                .map(Result::unwrap)
                .find(|e| e.file_name().to_string_lossy().starts_with(".zore-stage-"))
                .unwrap()
                .path()
                .join("tree")
        }
    }
    impl Drop for Sandbox {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn junction(link: &Path, target: &Path) {
        let output = Command::new("cmd.exe")
            .args(["/d", "/c", "mklink", "/J"])
            .arg(link)
            .arg(target)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{} {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    fn world_acl(path: &Path) {
        let text: Vec<u16> = "D:P(A;OICI;FA;;;WD)\0".encode_utf16().collect();
        let mut descriptor = std::ptr::null_mut();
        let mut size = 0;
        // SAFETY: descriptor conversion receives a terminated SDDL string and writable outputs.
        assert_ne!(
            unsafe {
                ConvertStringSecurityDescriptorToSecurityDescriptorW(
                    text.as_ptr(),
                    1,
                    &mut descriptor,
                    &mut size,
                )
            },
            0
        );
        let _memory = LocalMemory(descriptor);
        let mut path = wide(path.as_os_str());
        path.push(0);
        // SAFETY: the fixture is owned by this test and the security descriptor is valid.
        assert_ne!(
            unsafe { SetFileSecurityW(path.as_ptr(), 0x80000004, descriptor) },
            0,
            "{}",
            io::Error::last_os_error()
        );
    }

    #[test]
    fn windows_staged_tree_publication_and_protected_security() {
        assert_eq!(size_of::<ObjectAttributes>(), 48);
        assert_eq!(size_of::<UnicodeString>(), 16);
        assert_eq!(size_of::<IoStatus>(), 16);
        let root = Sandbox::new();
        let mut tree = Tree::create(&root.destination(), 3).unwrap();
        tree.mkdir("a").unwrap();
        tree.create_file("a/b.txt", false).unwrap();
        tree.security
            .verify(tree.envelope.as_ref().unwrap())
            .unwrap();
        tree.security.verify(tree.root.as_ref().unwrap()).unwrap();
        tree.security.verify(tree.active.as_ref().unwrap()).unwrap();
        tree.file().unwrap().write_all(b"hello").unwrap();
        assert!(!root.0.join("result").exists());
        tree.finish_file().unwrap();
        tree.mkdir("a").unwrap();
        tree.commit().unwrap();
        assert_eq!(
            std::fs::read(root.0.join("result/a/b.txt")).unwrap(),
            b"hello"
        );
        assert_eq!(root.children(), 1);
        let file = File::open(root.0.join("result/a/b.txt")).unwrap();
        tree.security.verify(&file).unwrap();
    }
    #[test]
    fn windows_staged_tree_blocks_inherited_acl_grants() {
        let root = Sandbox::new();
        world_acl(&root.0);
        let mut tree = Tree::create(&root.destination(), 2).unwrap();
        tree.create_file("dir/file", false).unwrap();
        tree.security
            .verify(tree.envelope.as_ref().unwrap())
            .unwrap();
        tree.security.verify(tree.root.as_ref().unwrap()).unwrap();
        tree.security.verify(tree.active.as_ref().unwrap()).unwrap();
        let visible = root.0.join("unprotected");
        std::fs::write(&visible, b"x").unwrap();
        world_acl(&visible);
        assert!(
            tree.security
                .verify(&File::open(&visible).unwrap())
                .is_err()
        );
        tree.finish_file().unwrap();
        tree.abort().unwrap();
        assert_eq!(root.children(), 1);
    }
    #[test]
    fn windows_staged_tree_rejects_reparse_points_and_ambiguous_roots() {
        let root = Sandbox::new();
        let outside = Sandbox::new();
        junction(&root.0.join("link"), &outside.0);
        assert!(Tree::create(root.0.join("link/result").to_str().unwrap(), 10).is_err());
        let mut tree = Tree::create(&root.destination(), 10).unwrap();
        let alias = root.staging().join("alias");
        junction(&alias, &outside.0);
        assert!(tree.create_file("alias/escape", false).is_err());
        assert_eq!(outside.children(), 0);
        std::fs::remove_dir(alias).unwrap();
        tree.abort().unwrap();
        for value in [
            "C:relative",
            "\\relative",
            "\\\\.\\C:\\result",
            "a/../b",
            "result/",
            ".",
            "..",
        ] {
            assert!(Tree::create(value, 1).is_err(), "{value}");
        }
        std::fs::remove_dir(root.0.join("link")).unwrap();
        assert_eq!(root.children(), 0);
    }
    #[test]
    fn windows_staged_tree_rejects_file_and_directory_symlinks() {
        let root = Sandbox::new();
        let outside = Sandbox::new();
        let target = outside.0.join("target");
        std::fs::write(&target, b"untouched").unwrap();
        std::os::windows::fs::symlink_file(&target, root.0.join("result"))
            .expect("Windows CI must allow symlink fixtures");
        assert!(Tree::create(&root.destination(), 1).is_err());
        std::fs::remove_file(root.0.join("result")).unwrap();
        std::os::windows::fs::symlink_dir(&outside.0, root.0.join("link"))
            .expect("Windows CI must allow symlink fixtures");
        assert!(Tree::create(root.0.join("link/result").to_str().unwrap(), 1).is_err());
        std::fs::remove_dir(root.0.join("link")).unwrap();
        assert_eq!(std::fs::read(target).unwrap(), b"untouched");
        assert_eq!(root.children(), 0);
    }
    #[test]
    fn windows_staged_tree_aliases_limits_and_drop_cleanup() {
        let root = Sandbox::new();
        {
            let mut tree = Tree::create(&root.destination(), 3).unwrap();
            tree.mkdir("LongDirectoryName").unwrap();
            assert!(tree.mkdir("LONGDIRECTORYNAME").is_err());
            let mut long = wide(root.staging().join("LongDirectoryName").as_os_str());
            long.push(0);
            // SAFETY: this size query uses a valid terminated filename and no output storage.
            let required = unsafe { GetShortPathNameW(long.as_ptr(), std::ptr::null_mut(), 0) };
            assert_ne!(required, 0, "{}", io::Error::last_os_error());
            let mut short = vec![0u16; required as usize];
            // SAFETY: the output buffer has the exact requested capacity.
            let count = unsafe { GetShortPathNameW(long.as_ptr(), short.as_mut_ptr(), required) };
            assert!(count > 0 && count < required);
            let short = String::from_utf16(&short[..count as usize]).unwrap();
            let alias = Path::new(&short).file_name().unwrap().to_str().unwrap();
            if !alias.eq_ignore_ascii_case("LongDirectoryName") {
                assert!(tree.create_file(&format!("{alias}/escape"), false).is_err());
            } else {
                eprintln!("8.3 aliases are disabled on this Windows volume");
            }
            assert!(tree.mkdir(&["a"; 65].join("/")).is_err());
            for name in [
                "../escape",
                "/absolute",
                "a//b",
                "a/./b",
                "a\\b",
                "a:stream",
                "a\0b",
            ] {
                assert!(tree.create_file(name, false).is_err(), "{name:?}");
            }
        }
        assert_eq!(root.children(), 0);
        let mut tree = Tree::create(&root.destination(), 2).unwrap();
        assert!(tree.create_file("a/b/c", false).is_err());
        tree.abort().unwrap();
        assert_eq!(root.children(), 0);
    }
    #[test]
    fn windows_staged_tree_commit_race_preserves_existing_destination() {
        let root = Sandbox::new();
        let mut tree = Tree::create(&root.destination(), 3).unwrap();
        tree.create_file("a/file", false).unwrap();
        tree.finish_file().unwrap();
        std::fs::write(root.0.join("result"), b"sentinel").unwrap();
        assert!(tree.commit().is_err());
        tree.abort().unwrap();
        assert_eq!(std::fs::read(root.0.join("result")).unwrap(), b"sentinel");
        assert_eq!(root.children(), 1);
        std::fs::remove_file(root.0.join("result")).unwrap();
        let mut tree = Tree::create(&root.destination(), 3).unwrap();
        tree.create_file("unfinished", false).unwrap();
        assert!(tree.commit().is_err());
        drop(tree);
        assert_eq!(root.children(), 0);
    }
    #[test]
    fn windows_staged_tree_parent_replacement_stays_anchored() {
        let root = Sandbox::new();
        let outside = Sandbox::new();
        std::fs::create_dir(root.0.join("parent")).unwrap();
        let (parent, destination) =
            parent_path(root.0.join("parent/result").to_str().unwrap()).unwrap();
        std::fs::rename(root.0.join("parent"), root.0.join("moved")).unwrap();
        junction(&root.0.join("parent"), &outside.0);
        let mut tree = Tree::from_parent(parent, destination, 3).unwrap();
        tree.create_file("file", false).unwrap();
        tree.file().unwrap().write_all(b"anchored").unwrap();
        tree.finish_file().unwrap();
        tree.commit().unwrap();
        assert_eq!(
            std::fs::read(root.0.join("moved/result/file")).unwrap(),
            b"anchored"
        );
        assert_eq!(outside.children(), 0);
        std::fs::remove_dir(root.0.join("parent")).unwrap();
    }
    #[test]
    fn windows_staged_tree_active_parent_cannot_redirect_publication() {
        let root = Sandbox::new();
        let outside = Sandbox::new();
        std::fs::create_dir(root.0.join("parent")).unwrap();
        let mut tree = Tree::create(root.0.join("parent/result").to_str().unwrap(), 3).unwrap();
        let moved = match std::fs::rename(root.0.join("parent"), root.0.join("moved")) {
            Ok(()) => {
                junction(&root.0.join("parent"), &outside.0);
                true
            }
            Err(error) if matches!(error.raw_os_error(), Some(5 | 32)) => {
                assert!(root.0.join("parent").is_dir());
                assert!(!root.0.join("moved").exists());
                let attempt = Command::new("cmd.exe")
                    .args(["/d", "/c", "mklink", "/J"])
                    .arg(root.0.join("parent"))
                    .arg(&outside.0)
                    .output()
                    .unwrap();
                assert!(!attempt.status.success());
                false
            }
            Err(error) => panic!("unexpected parent replacement error: {error}"),
        };
        tree.create_file("file", false).unwrap();
        tree.file().unwrap().write_all(b"anchored").unwrap();
        tree.finish_file().unwrap();
        tree.commit().unwrap();
        let destination = root.0.join(if moved {
            "moved/result/file"
        } else {
            "parent/result/file"
        });
        assert_eq!(std::fs::read(destination).unwrap(), b"anchored");
        assert_eq!(outside.children(), 0);
        if moved {
            std::fs::remove_dir(root.0.join("parent")).unwrap();
        }
    }
    #[test]
    fn windows_staged_tree_replacement_is_not_deleted_during_cleanup() {
        let root = Sandbox::new();
        let mut tree = Tree::create(&root.destination(), 2).unwrap();
        tree.create_file("file", false).unwrap();
        tree.finish_file().unwrap();
        let file = root.staging().join("file");
        std::fs::remove_file(&file).unwrap();
        std::fs::write(&file, b"replacement").unwrap();
        assert!(tree.abort().is_err());
        assert_eq!(std::fs::read(&file).unwrap(), b"replacement");
        std::fs::remove_file(file).unwrap();
        tree.abort().unwrap();
        assert_eq!(root.children(), 0);
    }
    #[test]
    fn windows_staged_tree_hard_links_share_files_and_symlinks_fail() {
        let root = Sandbox::new();
        let mut tree = Tree::create(&root.destination(), 5).unwrap();
        tree.create_file("dir/file", false).unwrap();
        tree.file().unwrap().write_all(b"x").unwrap();
        tree.finish_file().unwrap();
        tree.hard_link("hard", "dir/file").unwrap();
        assert!(tree.hard_link("missing", "dir/none").is_err());
        assert!(tree.symlink("soft", "dir/file").is_err());
        tree.commit().unwrap();
        assert_eq!(std::fs::read(root.0.join("result/hard")).unwrap(), b"x");
    }
    #[test]
    fn windows_staged_tree_canonical_paths_accept_mixed_separators() {
        let root = Sandbox::new();
        let destination = format!("{}/result", root.0.display());
        let mut tree = Tree::create(&destination, 1).unwrap();
        tree.create_file("é file", false).unwrap();
        tree.file().unwrap().write_all(b"canonical").unwrap();
        tree.finish_file().unwrap();
        tree.commit().unwrap();
        assert_eq!(
            std::fs::read(root.0.join("result/é file")).unwrap(),
            b"canonical"
        );
        assert_eq!(root.children(), 1);
    }
}
