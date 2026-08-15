//! Typed public API.
//!
//! Everything a consumer of this crate names lives here (plus the plain
//! data types re-exported from [`crate::wire`]). The wire types
//! (`WinScmRequest`/`WinScmResponse`/`WinScmExt`/`ScManagerMarker`/
//! `ServiceMarker`) are an implementation detail, same as
//! `dolang-vfs` never exposing its own `RequestKind`/`ResponseKind`/
//! `VfsProtocol`.

use dolang_vfs::extension::{ExtCite, ExtGift, VfsExtension};
use dolang_vfs::{
    AnyVfs, Vfs,
    error::{Error, ErrorKind},
};
use dolang_winterop::security::SecDesc;
use std::sync::{Arc, Weak};

use crate::wire::{
    CreateServiceOptions, ErrorControl, NotifyMask, ScManagerMarker, ServiceAccess, ServiceConfig,
    ServiceConfigUpdate, ServiceControl, ServiceInfo, ServiceMarker, ServiceStateFilter,
    ServiceStatus, ServiceType, StartType, WinScmExt, WinScmRequest, WinScmResponse,
};

/// A response variant didn't match what the request kind is documented to
/// return.
///
/// A mismatched response is untrusted wire input rather than a
/// locally-provable invariant: a buggy or misbehaving peer can send back any
/// variant it likes, so a caller here gets a real, catchable error instead of
/// a panic.
fn unexpected(request: &str) -> Error {
    Error::new(
        ErrorKind::Other,
        format!("unexpected response for {request}"),
    )
}

/// An open handle to the Service Control Manager database.
pub struct ScManager {
    vfs: AnyVfs,
    handle: Arc<ScManagerState>,
}

struct ScManagerState {
    vfs: AnyVfs,
    opaque: Option<ExtGift<ScManagerMarker>>,
}

impl ScManagerState {
    fn new(vfs: AnyVfs, opaque: ExtGift<ScManagerMarker>) -> Self {
        Self {
            vfs,
            opaque: Some(opaque),
        }
    }

    fn cite(&self) -> Result<ExtCite<ScManagerMarker>, Error> {
        self.opaque
            .as_ref()
            .map(ExtGift::cite)
            .ok_or_else(|| Error::new(ErrorKind::InvalidInput, "SC manager is closed"))
    }

    fn take_opaque(&mut self) -> ExtGift<ScManagerMarker> {
        self.opaque
            .take()
            .expect("live manager state has no opaque")
    }
}

impl Drop for ScManagerState {
    fn drop(&mut self) {
        let Some(manager) = self.opaque.take() else {
            return;
        };
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            return;
        };
        let vfs = self.vfs.clone();
        runtime.spawn(async move {
            let _ = vfs
                .call_extension::<WinScmExt>(WinScmRequest::CloseManager {
                    manager: manager.cite(),
                })
                .await;
        });
    }
}

impl ScManager {
    /// Opens the Service Control Manager database.
    pub async fn open(vfs: &AnyVfs, access: ServiceAccess) -> Result<ScManager, Error> {
        if vfs
            .extensions()
            .maximum_common_version(WinScmExt::NAME, &[WinScmExt::VERSION])
            .is_none()
        {
            return Err(Error::new(
                ErrorKind::Unsupported,
                "Windows Service Control Manager is not supported by this VFS backend",
            ));
        }
        let response = vfs
            .call_extension::<WinScmExt>(WinScmRequest::OpenManager { access })
            .await??;
        match response {
            WinScmResponse::Manager(handle) => Ok(ScManager {
                vfs: vfs.clone(),
                handle: Arc::new(ScManagerState::new(vfs.clone(), handle)),
            }),
            _ => Err(unexpected("OpenManager")),
        }
    }

    /// Opens an existing service by name.
    pub async fn open_service(&self, name: &str, access: ServiceAccess) -> Result<Service, Error> {
        let response = self
            .vfs
            .call_extension::<WinScmExt>(WinScmRequest::OpenService {
                manager: self.handle.cite()?,
                name: name.to_string(),
                access,
            })
            .await??;
        match response {
            WinScmResponse::Svc(handle) => Ok(Service {
                vfs: self.vfs.clone(),
                handle,
            }),
            _ => Err(unexpected("OpenService")),
        }
    }

    /// Creates a new service.
    #[allow(clippy::too_many_arguments)]
    pub async fn create_service(
        &self,
        name: &str,
        display_name: Option<&str>,
        service_type: ServiceType,
        start_type: StartType,
        error_control: ErrorControl,
        binary_path: Option<&str>,
        access: ServiceAccess,
    ) -> Result<Service, Error> {
        self.create_service_with_options(
            name,
            display_name,
            service_type,
            start_type,
            error_control,
            binary_path,
            CreateServiceOptions::default(),
            access,
        )
        .await
    }

    /// Creates a new service with optional dependencies, load-order group,
    /// and service account credentials.
    #[allow(clippy::too_many_arguments)]
    pub async fn create_service_with_options(
        &self,
        name: &str,
        display_name: Option<&str>,
        service_type: ServiceType,
        start_type: StartType,
        error_control: ErrorControl,
        binary_path: Option<&str>,
        options: CreateServiceOptions,
        access: ServiceAccess,
    ) -> Result<Service, Error> {
        let response = self
            .vfs
            .call_extension::<WinScmExt>(WinScmRequest::CreateService {
                manager: self.handle.cite()?,
                name: name.to_string(),
                display_name: display_name.map(str::to_owned),
                service_type,
                start_type,
                error_control,
                binary_path: binary_path.map(str::to_owned),
                options,
                access,
            })
            .await??;
        match response {
            WinScmResponse::Svc(handle) => Ok(Service {
                vfs: self.vfs.clone(),
                handle,
            }),
            _ => Err(unexpected("CreateService")),
        }
    }

    /// Opens a live forward enumeration of matching services.
    pub async fn enumerate_services(
        &self,
        service_type: ServiceType,
        state_filter: ServiceStateFilter,
    ) -> Result<Services, Error> {
        Ok(Services {
            vfs: self.vfs.clone(),
            manager: Arc::downgrade(&self.handle),
            service_type,
            state_filter,
            resume: 0,
            entries: std::collections::VecDeque::new(),
            done: false,
        })
    }

    /// Explicitly closes this manager handle.
    ///
    /// Not required for correctness — an abandoned or dropped `ScManager`
    /// is closed automatically (in remote mode, when the connection's
    /// opaque object table is torn down) — but lets a well-behaved caller
    /// observe close failures immediately.
    pub async fn close(self) -> Result<(), Error> {
        let mut state = Arc::try_unwrap(self.handle).map_err(|handle| {
            drop(handle);
            Error::new(
                ErrorKind::ResourceBusy,
                "SC manager has an operation in progress",
            )
        })?;
        let manager = state.take_opaque().cite();
        let response = self
            .vfs
            .call_extension::<WinScmExt>(WinScmRequest::CloseManager { manager })
            .await??;
        match response {
            WinScmResponse::Closed => Ok(()),
            _ => Err(unexpected("CloseManager")),
        }
    }
}

/// A live forward enumeration of services.
pub struct Services {
    vfs: AnyVfs,
    manager: Weak<ScManagerState>,
    service_type: ServiceType,
    state_filter: ServiceStateFilter,
    resume: u32,
    entries: std::collections::VecDeque<ServiceInfo>,
    done: bool,
}

impl Services {
    /// Returns the next matching service.
    pub async fn next_entry(&mut self) -> Result<Option<ServiceInfo>, Error> {
        let manager = self
            .manager
            .upgrade()
            .ok_or_else(|| Error::new(ErrorKind::InvalidInput, "SC manager is closed"))?;
        manager.cite()?;
        if let Some(entry) = self.entries.pop_front() {
            return Ok(Some(entry));
        }
        if self.done {
            return Ok(None);
        }
        let response = self
            .vfs
            .call_extension::<WinScmExt>(WinScmRequest::EnumServicesPage {
                manager: manager.cite()?,
                service_type: self.service_type,
                state_filter: self.state_filter,
                resume: self.resume,
            })
            .await??;
        let WinScmResponse::ServicesPage {
            services,
            resume,
            done,
        } = response
        else {
            return Err(unexpected("EnumServicesPage"));
        };
        self.resume = resume;
        self.done = done;
        self.entries = services.into();
        Ok(self.entries.pop_front())
    }
}

/// An open handle to a specific service.
pub struct Service {
    vfs: AnyVfs,
    handle: ExtGift<ServiceMarker>,
}

impl Service {
    /// Marks this service for deletion. It is actually removed once every
    /// open handle to it (including this one) is closed.
    pub async fn delete(&self) -> Result<(), Error> {
        let response = self
            .vfs
            .call_extension::<WinScmExt>(WinScmRequest::DeleteService {
                service: self.handle.cite(),
            })
            .await??;
        match response {
            WinScmResponse::Deleted => Ok(()),
            _ => Err(unexpected("DeleteService")),
        }
    }

    /// Explicitly closes this service handle.
    ///
    /// Not required for correctness — an abandoned or dropped `Service` is
    /// closed automatically (in remote mode, when the connection's opaque
    /// object table is torn down) — but lets a well-behaved caller observe
    /// close failures immediately.
    pub async fn close(self) -> Result<(), Error> {
        let response = self
            .vfs
            .call_extension::<WinScmExt>(WinScmRequest::CloseService {
                service: self.handle.cite(),
            })
            .await??;
        match response {
            WinScmResponse::Closed => Ok(()),
            _ => Err(unexpected("CloseService")),
        }
    }

    /// Starts the service without arguments.
    pub async fn start(&self) -> Result<(), Error> {
        self.start_with_args(&[]).await
    }

    /// Starts the service with arguments passed to its `ServiceMain`.
    pub async fn start_with_args(&self, args: &[String]) -> Result<(), Error> {
        let response = self
            .vfs
            .call_extension::<WinScmExt>(WinScmRequest::StartService {
                service: self.handle.cite(),
                args: args.to_vec(),
            })
            .await??;
        match response {
            WinScmResponse::Ack => Ok(()),
            _ => Err(unexpected("StartService")),
        }
    }

    /// Sends a control code to the service (e.g. stop, pause, continue),
    /// returning the resulting status.
    pub async fn control(&self, control: ServiceControl) -> Result<ServiceStatus, Error> {
        let response = self
            .vfs
            .call_extension::<WinScmExt>(WinScmRequest::ControlService {
                service: self.handle.cite(),
                control,
            })
            .await??;
        match response {
            WinScmResponse::Status(status) => Ok(status),
            _ => Err(unexpected("ControlService")),
        }
    }

    /// Queries the service's current status.
    pub async fn query_status(&self) -> Result<ServiceStatus, Error> {
        let response = self
            .vfs
            .call_extension::<WinScmExt>(WinScmRequest::QueryStatus {
                service: self.handle.cite(),
            })
            .await??;
        match response {
            WinScmResponse::Status(status) => Ok(status),
            _ => Err(unexpected("QueryStatus")),
        }
    }

    /// Fetches an immutable snapshot of the service's base configuration.
    pub async fn config(&self) -> Result<ServiceConfig, Error> {
        let response = self
            .vfs
            .call_extension::<WinScmExt>(WinScmRequest::QueryConfig {
                service: self.handle.cite(),
            })
            .await??;
        match response {
            WinScmResponse::Config(config) => Ok(config),
            _ => Err(unexpected("QueryConfig")),
        }
    }

    /// Changes the selected fields of the service's base configuration.
    pub async fn set_config(&self, update: ServiceConfigUpdate) -> Result<(), Error> {
        let response = self
            .vfs
            .call_extension::<WinScmExt>(WinScmRequest::ChangeConfig {
                service: self.handle.cite(),
                update,
            })
            .await??;
        match response {
            WinScmResponse::Ack => Ok(()),
            _ => Err(unexpected("ChangeConfig")),
        }
    }

    /// Fetches the service's security descriptor. `mask` selects which
    /// components to fetch (an OR of `dolang_vfs`'s
    /// `*_SECURITY_INFORMATION` constants); `0` fetches just the owner.
    ///
    /// Requesting the SACL component requires the service to have been opened
    /// with [`ServiceAccess::ACCESS_SYSTEM_SECURITY`]. Opening a service with
    /// that right requires `SeSecurityPrivilege`, which is enabled
    /// automatically for the open or create operation if available. Fetching
    /// through the resulting handle does not require the privilege to remain
    /// enabled. Fetching any other component requires
    /// [`ServiceAccess::READ_CONTROL`].
    pub async fn sec_desc(&self, mask: u32) -> Result<SecDesc, Error> {
        let response = self
            .vfs
            .call_extension::<WinScmExt>(WinScmRequest::GetSecDesc {
                service: self.handle.cite(),
                mask: dolang_winterop::security::SecInfo::from_bits_retain(mask),
            })
            .await??;
        match response {
            WinScmResponse::SecDesc(descriptor) => Ok(descriptor),
            _ => Err(unexpected("GetSecDesc")),
        }
    }

    /// Sets the service's security descriptor. Which components are
    /// updated is determined by `descriptor.mask()`.
    ///
    /// Setting the DACL/owner/group requires the service to have been
    /// opened with the corresponding
    /// [`ServiceAccess::WRITE_DAC`]/[`ServiceAccess::WRITE_OWNER`] right;
    /// setting the SACL requires [`ServiceAccess::ACCESS_SYSTEM_SECURITY`]
    /// plus `SeSecurityPrivilege` (elevated automatically for the duration
    /// of this call if available).
    pub async fn set_sec_desc(&self, descriptor: &SecDesc) -> Result<(), Error> {
        let response = self
            .vfs
            .call_extension::<WinScmExt>(WinScmRequest::SetSecDesc {
                service: self.handle.cite(),
                sec_desc: descriptor.clone(),
            })
            .await??;
        match response {
            WinScmResponse::Ack => Ok(()),
            _ => Err(unexpected("SetSecDesc")),
        }
    }

    /// Asynchronously waits for the service's status to change to one of
    /// the states in `mask`, returning the new status.
    ///
    /// Cancellable: dropping this call's future (or, under remote dispatch,
    /// the peer cancelling the in-flight request) stops the wait. See
    /// `dolang-winterop::apc` for how the underlying reactor makes this
    /// safe even though the Win32 notification API it wraps has a real
    /// memory-safety hazard on cancellation.
    pub async fn wait_for_status_change(&self, mask: NotifyMask) -> Result<ServiceStatus, Error> {
        let response = self
            .vfs
            .call_extension::<WinScmExt>(WinScmRequest::WaitForStatusChange {
                service: self.handle.cite(),
                mask,
            })
            .await??;
        match response {
            WinScmResponse::Status(status) => Ok(status),
            _ => Err(unexpected("WaitForStatusChange")),
        }
    }
}
