use std::{ffi::OsStr, os::windows::ffi::OsStrExt, ptr, slice};

use dolang_vfs::target::OperatingSystem;
use dolang_vfs::{
    error::{Error, ErrorKind},
    extension::ExtContext,
};
use dolang_winterop::security::Sid;
use windows_sys::Win32::{
    Foundation::{
        ERROR_ACCESS_DENIED, ERROR_INVALID_PARAMETER, ERROR_INVALID_PASSWORD, ERROR_MORE_DATA,
    },
    NetworkManagement::NetManagement::{
        FILTER_NORMAL_ACCOUNT, MAX_PREFERRED_LENGTH, NERR_PasswordTooShort, NERR_Success,
        NERR_UserExists, NERR_UserNotFound, NetApiBufferFree, NetUserAdd, NetUserDel, NetUserEnum,
        NetUserGetInfo, NetUserSetInfo, USER_INFO_0, USER_INFO_1, USER_INFO_4, USER_INFO_1003,
        USER_INFO_1006, USER_INFO_1007, USER_INFO_1008, USER_INFO_1009, USER_INFO_1011,
        USER_INFO_1012, USER_INFO_1017, USER_INFO_1052, USER_INFO_1053,
    },
    Security::GetLengthSid,
};

use crate::wire::{UserCreate, UserFlags, UserInfo, UserUpdate, WinNetRequest, WinNetResponse};

struct NetBuffer(*mut u8);
impl Drop for NetBuffer {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { NetApiBufferFree(self.0.cast()) };
        }
    }
}

fn wide(value: &str) -> Vec<u16> {
    OsStr::new(value).encode_wide().chain(Some(0)).collect()
}
unsafe fn string(ptr: *const u16) -> String {
    if ptr.is_null() {
        return String::new();
    }
    let len = unsafe { (0..).take_while(|&i| *ptr.add(i) != 0).count() };
    String::from_utf16_lossy(unsafe { slice::from_raw_parts(ptr, len) })
}
unsafe fn optional(ptr: *const u16) -> Option<String> {
    let value = unsafe { string(ptr) };
    (!value.is_empty()).then_some(value)
}

fn status(operation: &str, code: u32) -> Error {
    let kind = if code == NERR_UserNotFound {
        ErrorKind::NotFound
    } else if code == NERR_UserExists {
        ErrorKind::AlreadyExists
    } else if code == ERROR_ACCESS_DENIED {
        ErrorKind::PermissionDenied
    } else if code == ERROR_INVALID_PARAMETER
        || code == ERROR_INVALID_PASSWORD
        || code == NERR_PasswordTooShort
    {
        ErrorKind::InvalidInput
    } else {
        ErrorKind::Other
    };
    Error::from_system_code(
        kind,
        format!("{operation}: NetAPI status {code}"),
        OperatingSystem::Windows,
        code as i32,
    )
}

unsafe fn sid_from_raw(raw: *mut core::ffi::c_void) -> Result<Sid, Error> {
    let len = unsafe { GetLengthSid(raw) } as usize;
    Sid::from_bytes(unsafe { slice::from_raw_parts(raw.cast(), len) })
        .map_err(|e| Error::new(ErrorKind::InvalidData, e))
}

fn get(name: &str) -> Result<(Sid, UserInfo), Error> {
    let name_w = wide(name);
    let mut raw = ptr::null_mut();
    let code = unsafe { NetUserGetInfo(ptr::null(), name_w.as_ptr(), 4, &mut raw) };
    if code != NERR_Success {
        return Err(status("NetUserGetInfo", code));
    }
    let buffer = NetBuffer(raw);
    let info = unsafe { &*buffer.0.cast::<USER_INFO_4>() };
    let sid = unsafe { sid_from_raw(info.usri4_user_sid) }?;
    let result = UserInfo {
        name: unsafe { string(info.usri4_name) },
        full_name: unsafe { optional(info.usri4_full_name) },
        comment: unsafe { optional(info.usri4_comment) },
        user_comment: unsafe { optional(info.usri4_usr_comment) },
        home_dir: unsafe { optional(info.usri4_home_dir) },
        home_dir_drive: unsafe { optional(info.usri4_home_dir_drive) },
        profile: unsafe { optional(info.usri4_profile) },
        script_path: unsafe { optional(info.usri4_script_path) },
        flags: UserFlags::from_bits_retain(info.usri4_flags),
        password_age: u64::from(info.usri4_password_age),
        password_expired: info.usri4_password_expired != 0,
        last_logon: (info.usri4_last_logon != 0).then_some(u64::from(info.usri4_last_logon)),
        account_expires: (info.usri4_acct_expires != u32::MAX)
            .then_some(u64::from(info.usri4_acct_expires)),
        bad_password_count: info.usri4_bad_pw_count,
        logon_count: info.usri4_num_logons,
    };
    Ok((sid, result))
}

fn verified(name: &str, expected: &Sid) -> Result<UserInfo, Error> {
    let (actual, info) = get(name)?;
    if &actual != expected {
        return Err(Error::new(
            ErrorKind::NotFound,
            "cached user name now identifies a different SID",
        ));
    }
    Ok(info)
}

fn set<T>(name: &str, level: u32, value: &T) -> Result<(), Error> {
    let name = wide(name);
    let mut parm = 0;
    let code = unsafe {
        NetUserSetInfo(
            ptr::null(),
            name.as_ptr(),
            level,
            (value as *const T).cast(),
            &mut parm,
        )
    };
    if code == NERR_Success {
        Ok(())
    } else {
        Err(status(
            &format!("NetUserSetInfo level {level} parameter {parm}"),
            code,
        ))
    }
}

fn update(name: &str, expected: &Sid, update: UserUpdate) -> Result<UserInfo, Error> {
    let current = verified(name, expected)?;
    let set_flags = update.set_flags();
    let clear_flags = update.clear_flags();
    macro_rules! text {
        ($field:ident, $level:expr, $ty:ident, $member:ident) => {
            if let Some(value) = update.$field {
                let mut w = wide(value.as_deref().unwrap_or(""));
                set(
                    name,
                    $level,
                    &$ty {
                        $member: w.as_mut_ptr(),
                    },
                )?;
            }
        };
    }
    if let Some(password) = update.password {
        let mut w = wide(&password);
        set(
            name,
            1003,
            &USER_INFO_1003 {
                usri1003_password: w.as_mut_ptr(),
            },
        )?;
    }
    text!(home_dir, 1006, USER_INFO_1006, usri1006_home_dir);
    text!(comment, 1007, USER_INFO_1007, usri1007_comment);
    text!(script_path, 1009, USER_INFO_1009, usri1009_script_path);
    text!(full_name, 1011, USER_INFO_1011, usri1011_full_name);
    text!(user_comment, 1012, USER_INFO_1012, usri1012_usr_comment);
    text!(profile, 1052, USER_INFO_1052, usri1052_profile);
    text!(
        home_dir_drive,
        1053,
        USER_INFO_1053,
        usri1053_home_dir_drive
    );
    if let Some(expires) = update.account_expires {
        let value = expires.map_or(u32::MAX, |v| u32::try_from(v).unwrap_or(u32::MAX));
        set(
            name,
            1017,
            &USER_INFO_1017 {
                usri1017_acct_expires: value,
            },
        )?;
    }
    if !set_flags.is_empty() || !clear_flags.is_empty() {
        let flags = (current.flags | set_flags) & !clear_flags;
        set(
            name,
            1008,
            &USER_INFO_1008 {
                usri1008_flags: flags.bits(),
            },
        )?;
    }
    verified(name, expected)
}

fn create(create: UserCreate) -> Result<(String, Sid), Error> {
    let mut name = wide(&create.name);
    let mut password = wide(&create.password);
    let mut home = wide(
        create
            .update
            .home_dir
            .as_ref()
            .and_then(|v| v.as_deref())
            .unwrap_or(""),
    );
    let mut comment = wide(
        create
            .update
            .comment
            .as_ref()
            .and_then(|v| v.as_deref())
            .unwrap_or(""),
    );
    let mut script = wide(
        create
            .update
            .script_path
            .as_ref()
            .and_then(|v| v.as_deref())
            .unwrap_or(""),
    );
    let flags = ((UserFlags::NORMAL_ACCOUNT | create.update.set_flags())
        & !create.update.clear_flags())
    .bits();
    let raw = USER_INFO_1 {
        usri1_name: name.as_mut_ptr(),
        usri1_password: password.as_mut_ptr(),
        usri1_password_age: 0,
        usri1_priv: 1,
        usri1_home_dir: home.as_mut_ptr(),
        usri1_comment: comment.as_mut_ptr(),
        usri1_flags: flags,
        usri1_script_path: script.as_mut_ptr(),
    };
    let mut parm = 0;
    let code = unsafe {
        NetUserAdd(
            ptr::null(),
            1,
            (&raw as *const USER_INFO_1).cast(),
            &mut parm,
        )
    };
    if code != NERR_Success {
        return Err(status(&format!("NetUserAdd parameter {parm}"), code));
    }
    let (sid, _) = match get(&create.name) {
        Ok(user) => user,
        Err(error) => {
            unsafe { NetUserDel(ptr::null(), name.as_ptr()) };
            return Err(error);
        }
    };
    if let Err(error) = update(&create.name, &sid, create.update) {
        unsafe { NetUserDel(ptr::null(), name.as_ptr()) };
        return Err(error);
    }
    Ok((create.name, sid))
}

fn dispatch(request: WinNetRequest) -> Result<WinNetResponse, Error> {
    match request {
        WinNetRequest::UserByName { name } => {
            let (sid, info) = get(&name)?;
            Ok(WinNetResponse::User {
                name: info.name,
                sid,
            })
        }
        WinNetRequest::UserBySid { sid } => Err(Error::new(
            ErrorKind::NotFound,
            format!("SID {sid} requires VFS name resolution"),
        )),
        WinNetRequest::UsersPage { mut resume } => {
            let mut raw = ptr::null_mut();
            let mut read = 0;
            let mut total = 0;
            let code = unsafe {
                NetUserEnum(
                    ptr::null(),
                    0,
                    FILTER_NORMAL_ACCOUNT,
                    &mut raw,
                    MAX_PREFERRED_LENGTH,
                    &mut read,
                    &mut total,
                    &mut resume,
                )
            };
            if code != NERR_Success && code != ERROR_MORE_DATA {
                return Err(status("NetUserEnum", code));
            }
            let buffer = NetBuffer(raw);
            let rows =
                unsafe { slice::from_raw_parts(buffer.0.cast::<USER_INFO_0>(), read as usize) };
            let mut users = Vec::with_capacity(rows.len());
            for row in rows {
                let name = unsafe { string(row.usri0_name) };
                let (sid, _) = get(&name)?;
                users.push((name, sid));
            }
            Ok(WinNetResponse::UsersPage {
                users,
                resume,
                done: code == NERR_Success,
            })
        }
        WinNetRequest::CreateUser(options) => {
            let (name, sid) = create(options)?;
            Ok(WinNetResponse::User { name, sid })
        }
        WinNetRequest::Info { name, sid } => Ok(WinNetResponse::Info(verified(&name, &sid)?)),
        WinNetRequest::Update {
            name,
            sid,
            update: patch,
        } => Ok(WinNetResponse::Info(update(&name, &sid, patch)?)),
        WinNetRequest::Delete { name, sid } => {
            verified(&name, &sid)?;
            let n = wide(&name);
            let code = unsafe { NetUserDel(ptr::null(), n.as_ptr()) };
            if code != NERR_Success {
                return Err(status("NetUserDel", code));
            }
            Ok(WinNetResponse::Deleted)
        }
    }
}

pub(crate) async fn handle(
    _ctx: &mut ExtContext<'_>,
    request: WinNetRequest,
) -> Result<WinNetResponse, Error> {
    tokio::task::spawn_blocking(move || dispatch(request))
        .await
        .map_err(|e| Error::new(ErrorKind::Other, e))?
}
