//! Windows access control for the service: the descriptors of its pipes and
//! folders, the check of the data folder and the elevation check.

use std::ffi::c_void;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::fs::MetadataExt;
use std::path::Path;
use windows::core::{HSTRING, PCWSTR};
use windows::Win32::Foundation::{CloseHandle, LocalFree, HANDLE, HLOCAL};
use windows::Win32::Security::Authorization::{
    ConvertStringSecurityDescriptorToSecurityDescriptorW, GetNamedSecurityInfoW, SDDL_REVISION_1,
    SE_FILE_OBJECT,
};
use windows::Win32::Security::{
    GetAce, GetTokenInformation, IsWellKnownSid, TokenElevation, WinBuiltinAdministratorsSid,
    WinLocalSystemSid, ACCESS_ALLOWED_ACE, ACE_HEADER, ACL, DACL_SECURITY_INFORMATION,
    OWNER_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID, SECURITY_ATTRIBUTES, TOKEN_ELEVATION,
    TOKEN_QUERY,
};
use windows::Win32::Storage::FileSystem::CreateDirectoryW;
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

/// The admin pipe: SYSTEM and Administrators, nobody else, and nobody who
/// came over the network.
pub const ADMIN_PIPE_SDDL: &str = "D:P(D;;GA;;;NU)(A;;GA;;;SY)(A;;GA;;;BA)";
/// The status pipe: any signed-in local user may read it. Reading is all the
/// pipe offers; the right to create further instances stays with SYSTEM and
/// Administrators.
pub const STATUS_PIPE_SDDL: &str = "D:P(D;;GA;;;NU)(A;;GA;;;SY)(A;;GA;;;BA)(A;;GR;;;AU)";
/// The data folder and the folder above it: owned by Administrators, full
/// access for SYSTEM and Administrators, inherited by everything inside, and
/// nothing inherited from `%ProgramData%`, where every user may create files.
pub const PRIVATE_FOLDER_SDDL: &str = "O:BAD:PAI(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)";

const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
const ACCESS_ALLOWED_ACE_TYPE: u8 = 0;
const ACCESS_DENIED_ACE_TYPE: u8 = 1;
/// Every right that lets its holder change a folder, what is in it or who
/// may access it: add file, add subfolder, write extended attributes, delete
/// child, write attributes, delete, write DAC, write owner, and the generic
/// rights that include them (generic write, generic all).
const WRITE_RIGHTS: u32 = 0x2
    | 0x4
    | 0x10
    | 0x40
    | 0x100
    | 0x0001_0000
    | 0x0004_0000
    | 0x0008_0000
    | 0x4000_0000
    | 0x1000_0000;

/// A security descriptor built from SDDL text, released on drop.
pub struct Descriptor(PSECURITY_DESCRIPTOR);

impl Descriptor {
    pub fn from_sddl(sddl: &str) -> Result<Self, String> {
        let mut descriptor = PSECURITY_DESCRIPTOR::default();
        // SAFETY: the text is a valid HSTRING that outlives the call and
        // `descriptor` is a valid out-pointer. On success the system
        // allocates the descriptor, and Drop releases it with LocalFree.
        unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                &HSTRING::from(sddl),
                SDDL_REVISION_1,
                &mut descriptor,
                None,
            )
        }
        .map_err(|e| format!("cannot build a security descriptor: {e}"))?;
        Ok(Self(descriptor))
    }

    /// Attributes for creating one object; valid while `self` lives.
    pub fn attributes(&self) -> SECURITY_ATTRIBUTES {
        SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: self.0 .0,
            bInheritHandle: false.into(),
        }
    }
}

impl Drop for Descriptor {
    fn drop(&mut self) {
        // SAFETY: the pointer came from a system call that allocates with
        // LocalAlloc, and it is released exactly once.
        unsafe {
            let _ = LocalFree(Some(HLOCAL(self.0 .0)));
        }
    }
}

fn wide(path: &Path) -> Vec<u16> {
    path.as_os_str().encode_wide().chain([0]).collect()
}

/// Whether the path itself is a link or a junction, which could lead
/// anywhere.
fn is_reparse_point(path: &Path) -> Result<bool, String> {
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|e| format!("cannot look at {}: {e}", path.display()))?;
    if !metadata.is_dir() {
        return Err(format!("{} is not a folder", path.display()));
    }
    Ok(metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0)
}

/// Creates a folder that only SYSTEM and Administrators can use. The caller
/// must be an administrator: the descriptor names Administrators as owner.
pub fn create_private_folder(path: &Path) -> Result<(), String> {
    let descriptor = Descriptor::from_sddl(PRIVATE_FOLDER_SDDL)?;
    let attributes = descriptor.attributes();
    let path_wide = wide(path);
    // SAFETY: `path_wide` is a NUL-terminated string and `attributes` points
    // at a descriptor; both live until the call returns.
    unsafe { CreateDirectoryW(PCWSTR(path_wide.as_ptr()), Some(&attributes)) }
        .map_err(|e| format!("cannot create {}: {e}", path.display()))
}

/// Who an entry of an access list, or the owner, is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trustee {
    System,
    Administrators,
    Other,
}

/// One entry of a folder's access list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Entry {
    Allow {
        trustee: Trustee,
        rights: u32,
    },
    Deny,
    /// An entry type this check does not understand.
    Unknown,
}

/// The part of a folder's security that decides who can change it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FolderSecurity {
    pub owner: Trustee,
    /// None: the folder has no access list, which lets everyone in.
    pub entries: Option<Vec<Entry>>,
}

impl FolderSecurity {
    /// Ok when nobody but SYSTEM and Administrators can write to the folder
    /// or change who can.
    pub fn judge(&self) -> Result<(), &'static str> {
        if self.owner == Trustee::Other {
            // An owner can always rewrite the access list.
            return Err("its owner is neither SYSTEM nor Administrators");
        }
        let Some(entries) = &self.entries else {
            return Err("it has no access list");
        };
        for entry in entries {
            match entry {
                Entry::Allow {
                    trustee: Trustee::Other,
                    rights,
                } if rights & WRITE_RIGHTS != 0 => {
                    return Err("someone other than SYSTEM and Administrators can write to it");
                }
                Entry::Unknown => return Err("its access list has an entry of an unknown kind"),
                _ => {}
            }
        }
        Ok(())
    }
}

fn trustee(sid: PSID) -> Trustee {
    // SAFETY: both callers pass a SID inside a security descriptor that the
    // system returned and that is still allocated.
    unsafe {
        if IsWellKnownSid(sid, WinLocalSystemSid).as_bool() {
            Trustee::System
        } else if IsWellKnownSid(sid, WinBuiltinAdministratorsSid).as_bool() {
            Trustee::Administrators
        } else {
            Trustee::Other
        }
    }
}

/// Reads the owner and the access list of a folder.
pub fn read_folder_security(path: &Path) -> Result<FolderSecurity, String> {
    let path_wide = wide(path);
    let mut owner = PSID::default();
    let mut dacl: *mut ACL = std::ptr::null_mut();
    let mut descriptor = PSECURITY_DESCRIPTOR::default();
    // SAFETY: `path_wide` is NUL-terminated and the three out-pointers are
    // valid. On success `owner` and `dacl` point into `descriptor`, which the
    // system allocated and which is released below.
    let status = unsafe {
        GetNamedSecurityInfoW(
            PCWSTR(path_wide.as_ptr()),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
            Some(&mut owner),
            None,
            Some(&mut dacl),
            None,
            &mut descriptor,
        )
    };
    if status.is_err() {
        return Err(format!(
            "cannot read the access list of {}: error {}",
            path.display(),
            status.0
        ));
    }
    // Releases the descriptor on every way out of this function.
    let _descriptor = Descriptor(descriptor);

    let entries = if dacl.is_null() {
        None
    } else {
        // SAFETY: `dacl` is non-null and points at the access list inside
        // the descriptor returned above.
        let count = unsafe { (*dacl).AceCount };
        let mut entries = Vec::with_capacity(count as usize);
        for index in 0..u32::from(count) {
            let mut ace: *mut c_void = std::ptr::null_mut();
            // SAFETY: `dacl` is valid and `index` is below its entry count;
            // `ace` is a valid out-pointer.
            unsafe { GetAce(dacl, index, &mut ace) }
                .map_err(|e| format!("cannot read an access list entry: {e}"))?;
            // SAFETY: every entry starts with an ACE_HEADER.
            let kind = unsafe { (*(ace as *const ACE_HEADER)).AceType };
            entries.push(match kind {
                ACCESS_ALLOWED_ACE_TYPE => {
                    let allowed = ace as *const ACCESS_ALLOWED_ACE;
                    // SAFETY: an entry of this type is an ACCESS_ALLOWED_ACE:
                    // the rights, then the SID, which starts at SidStart.
                    let (rights, sid) = unsafe {
                        (
                            (*allowed).Mask,
                            PSID(std::ptr::addr_of!((*allowed).SidStart) as *mut c_void),
                        )
                    };
                    Entry::Allow {
                        trustee: trustee(sid),
                        rights,
                    }
                }
                ACCESS_DENIED_ACE_TYPE => Entry::Deny,
                _ => Entry::Unknown,
            });
        }
        Some(entries)
    };
    Ok(FolderSecurity {
        owner: trustee(owner),
        entries,
    })
}

/// Checks that a folder is what the service may trust with its settings and
/// logs: a real folder that only SYSTEM and Administrators can change.
pub fn check_private_folder(path: &Path) -> Result<(), String> {
    if is_reparse_point(path)? {
        return Err(format!("{} is a link, not a folder", path.display()));
    }
    read_folder_security(path)?
        .judge()
        .map_err(|reason| format!("{} cannot be trusted: {reason}", path.display()))
}

/// A process token handle, closed on drop.
struct Token(HANDLE);

impl Drop for Token {
    fn drop(&mut self) {
        // SAFETY: the handle came from OpenProcessToken and is closed once.
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

/// Whether this process runs with administrator rights that are switched on.
pub fn is_elevated() -> bool {
    let mut handle = HANDLE::default();
    // SAFETY: the pseudo handle of the current process is always valid and
    // `handle` is a valid out-pointer.
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut handle) }.is_err() {
        return false;
    }
    let token = Token(handle);
    let mut elevation = TOKEN_ELEVATION::default();
    let mut returned = 0u32;
    // SAFETY: the buffer is a TOKEN_ELEVATION of the size passed, and
    // `returned` is a valid out-pointer.
    let read = unsafe {
        GetTokenInformation(
            token.0,
            TokenElevation,
            Some(std::ptr::addr_of_mut!(elevation).cast()),
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut returned,
        )
    };
    read.is_ok() && elevation.TokenIsElevated != 0
}

#[cfg(test)]
mod tests {
    use super::*;

    const FULL: u32 = 0x001F_01FF;
    const READ_AND_EXECUTE: u32 = 0x0012_00A9;

    fn allow(trustee: Trustee, rights: u32) -> Entry {
        Entry::Allow { trustee, rights }
    }

    fn folder(owner: Trustee, entries: &[Entry]) -> FolderSecurity {
        FolderSecurity {
            owner,
            entries: Some(entries.to_vec()),
        }
    }

    #[test]
    fn a_folder_for_system_and_administrators_is_trusted() {
        let private = [
            allow(Trustee::System, FULL),
            allow(Trustee::Administrators, FULL),
        ];
        assert_eq!(folder(Trustee::Administrators, &private).judge(), Ok(()));
        assert_eq!(folder(Trustee::System, &private).judge(), Ok(()));
        // Others may look, and an entry that takes rights away does no harm.
        let readable = [
            allow(Trustee::System, FULL),
            allow(Trustee::Other, READ_AND_EXECUTE),
            Entry::Deny,
        ];
        assert_eq!(folder(Trustee::Administrators, &readable).judge(), Ok(()));
    }

    #[test]
    fn a_folder_someone_else_can_change_is_refused() {
        let private = [allow(Trustee::System, FULL)];
        assert!(folder(Trustee::Other, &private).judge().is_err(), "owner");
        assert!(
            FolderSecurity {
                owner: Trustee::Administrators,
                entries: None
            }
            .judge()
            .is_err(),
            "no access list"
        );
        // Each right that changes the folder is enough on its own.
        for right in [
            0x2,
            0x4,
            0x10,
            0x40,
            0x100,
            0x0001_0000,
            0x0004_0000,
            0x0008_0000,
            0x4000_0000,
            0x1000_0000,
        ] {
            let entries = [
                allow(Trustee::System, FULL),
                allow(Trustee::Other, READ_AND_EXECUTE | right),
            ];
            assert!(
                folder(Trustee::Administrators, &entries).judge().is_err(),
                "{right:#x}"
            );
        }
        let unknown = [allow(Trustee::System, FULL), Entry::Unknown];
        assert!(folder(Trustee::Administrators, &unknown).judge().is_err());
    }

    #[test]
    fn the_descriptors_are_valid_sddl() {
        for sddl in [ADMIN_PIPE_SDDL, STATUS_PIPE_SDDL, PRIVATE_FOLDER_SDDL] {
            Descriptor::from_sddl(sddl).unwrap();
        }
        assert!(Descriptor::from_sddl("not sddl").is_err());
    }

    #[test]
    fn a_folder_in_the_users_temp_folder_is_refused() {
        let dir =
            std::env::temp_dir().join(format!("deskmate-service-acl-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // It inherits the user's own full access from the temp folder.
        let security = read_folder_security(&dir).unwrap();
        assert!(security
            .entries
            .as_ref()
            .is_some_and(|entries| !entries.is_empty()));
        assert!(check_private_folder(&dir).is_err());
        assert!(check_private_folder(&dir.join("missing")).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn reads_the_access_list_a_folder_was_created_with() {
        let dir = std::env::temp_dir().join(format!(
            "deskmate-service-acl-read-test-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir(&dir);
        // The descriptor of the data folder, without its owner (only an
        // administrator may name Administrators as owner) and with full
        // access for whoever owns the folder, so the test can remove it.
        let descriptor =
            Descriptor::from_sddl("D:PAI(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;FA;;;OW)").unwrap();
        let attributes = descriptor.attributes();
        let path_wide = wide(&dir);
        // SAFETY: `path_wide` is NUL-terminated and `attributes` points at a
        // descriptor; both live until the call returns.
        unsafe { CreateDirectoryW(PCWSTR(path_wide.as_ptr()), Some(&attributes)) }.unwrap();

        let security = read_folder_security(&dir).unwrap();
        assert_eq!(
            security.entries,
            Some(vec![
                allow(Trustee::System, FULL),
                allow(Trustee::Administrators, FULL),
                allow(Trustee::Other, FULL),
            ])
        );
        // The third entry is what makes this folder one that is refused.
        assert!(check_private_folder(&dir).is_err_and(|error| error
            .contains("other than SYSTEM and Administrators")
            || error.contains("owner")));
        std::fs::remove_dir(&dir).unwrap();
    }
}
