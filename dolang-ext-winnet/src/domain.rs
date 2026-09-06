use dolang::runtime::{
    Error, Object, Output, Result, State, Strand, Value, object::TypeBuilder, unpack, value::Nil,
    vm::ModuleBuilder,
};
use dolang_ext_shell::{ResultExt, as_windows_path};
use dolang_vfs_winnet::domain;

use crate::global::Global;

pub(crate) struct JoinStatus;

pub(crate) struct JoinStatusAnnex<'v> {
    global: State<'v, Global<'v>>,
    status: domain::Status,
}

fn make_status<'v>(
    strand: &mut Strand<'v, '_>,
    global: State<'v, Global<'v>>,
    status: domain::Status,
    out: impl Output<'v>,
) {
    global.types.join_status.create_with_annex(
        strand,
        JoinStatus,
        JoinStatusAnnex { global, status },
        out,
    );
}

/// Coerces a required `Str` argument.
fn string<'v, 's>(
    strand: &mut Strand<'v, 's>,
    value: &Value<'v>,
    name: &str,
) -> Result<'v, 's, String> {
    value
        .as_str(strand)
        .map(Into::into)
        .ok_or_else(|| Error::type_error(strand, format!("{name} must be a Str")))
}

/// Coerces a `Str` or `nil` argument.
fn optional_string<'v, 's>(
    strand: &mut Strand<'v, 's>,
    value: &Value<'v>,
    name: &str,
) -> Result<'v, 's, Option<String>> {
    if value.is_nil() {
        return Ok(None);
    }
    string(strand, value, name).map(Some)
}

/// Coerces a `Bool` option flag.
fn flag<'v, 's>(
    strand: &mut Strand<'v, 's>,
    value: &Value<'v>,
    name: &str,
) -> Result<'v, 's, bool> {
    value
        .as_bool(strand)
        .ok_or_else(|| Error::type_error(strand, format!("{name} must be a Bool")))
}

/// Applies each supplied boolean option to a request builder.
///
/// Every option here is an independent flag with an identically shaped
/// setter, so spelling out one `if let` apiece would be nothing but noise.
macro_rules! options {
    ($request:ident, $strand:ident, $($value:ident),* $(,)?) => {
        $(if let Some(value) = $value {
            $request = $request.$value(flag($strand, &value, stringify!($value))?);
        })*
    };
}

/// Coerces the `account`/`password` pair, which is meaningful only together.
fn credentials<'v, 's>(
    strand: &mut Strand<'v, 's>,
    account: Option<&Value<'v>>,
    password: Option<&Value<'v>>,
) -> Result<'v, 's, Option<(String, String)>> {
    match (account, password) {
        (None, None) => Ok(None),
        (Some(account), Some(password)) => Ok(Some((
            string(strand, account, "account")?,
            string(strand, password, "password")?,
        ))),
        _ => Err(Error::value(
            strand,
            "account and password must be given together",
        )),
    }
}

impl<'v> Object<'v> for JoinStatus {
    const NAME: &'v str = "JoinStatus";
    const MODULE: &'v str = "winnet";
    type Annex = JoinStatusAnnex<'v>;
    type Type = ();
    type TypeAnnex = ();
    fn build<'a>(builder: TypeBuilder<'v, 'a, Self>) -> TypeBuilder<'v, 'a, Self> {
        builder
            .get("kind", |this, strand, out| {
                let annex = this.annex();
                let global = annex.global;
                let kind = match annex.status.kind() {
                    domain::Kind::Unjoined => global.syms.kind_unjoined,
                    domain::Kind::Workgroup => global.syms.kind_workgroup,
                    domain::Kind::Domain => global.syms.kind_domain,
                    domain::Kind::Unknown => global.syms.kind_unknown,
                };
                Output::set(strand, out, kind);
                Ok(())
            })
            .get("name", |this, strand, out| {
                match this.annex().status.name() {
                    Some(name) => Output::set(strand, out, name),
                    None => Output::set(strand, out, Nil),
                }
                Ok(())
            })
    }
}

pub(crate) fn configure_module<'v, 'a>(
    module: ModuleBuilder<'v, 'a>,
    global: State<'v, Global<'v>>,
) -> ModuleBuilder<'v, 'a> {
    module
        .value("JoinStatus", global.types.join_status)
        .function("join_status", async move |strand, args, out| {
            let ([], []) = unpack!(strand, args, 0, 0)?;
            let status = domain::status(&dolang_ext_shell::vfs(strand))
                .await
                .into_sys(strand)?;
            make_status(strand, global, status, out);
            Ok(())
        })
        .function("join_domain", async move |strand, args, out| {
            let ou_sym = global.syms.ou;
            let account_sym = global.syms.account;
            let password_sym = global.syms.password;
            let machine_password_sym = global.syms.machine_password;
            let create_account_sym = global.syms.create_account;
            let join_if_joined_sym = global.syms.join_if_joined;
            let unsecure_sym = global.syms.unsecure;
            let defer_spn_sym = global.syms.defer_spn;
            let force_spn_sym = global.syms.force_spn;
            let dc_account_sym = global.syms.dc_account;
            let with_new_name_sym = global.syms.with_new_name;
            let readonly_sym = global.syms.readonly;
            let ambiguous_dc_sym = global.syms.ambiguous_dc;
            let no_netlogon_cache_sym = global.syms.no_netlogon_cache;
            let no_account_reuse_sym = global.syms.no_account_reuse;
            let (
                [name],
                [
                    ou,
                    account,
                    password,
                    machine_password,
                    create_account,
                    join_if_joined,
                    unsecure,
                    defer_spn,
                    force_spn,
                    dc_account,
                    with_new_name,
                    readonly,
                    ambiguous_dc,
                    no_netlogon_cache,
                    no_account_reuse,
                ],
            ) = unpack!(
                strand,
                args,
                1,
                0,
                ou_sym = None,
                account_sym = None,
                password_sym = None,
                machine_password_sym = None,
                create_account_sym = None,
                join_if_joined_sym = None,
                unsecure_sym = None,
                defer_spn_sym = None,
                force_spn_sym = None,
                dc_account_sym = None,
                with_new_name_sym = None,
                readonly_sym = None,
                ambiguous_dc_sym = None,
                no_netlogon_cache_sym = None,
                no_account_reuse_sym = None
            )?;
            let mut request = domain::Join::new(string(strand, &name, "domain")?);
            if let Some(value) = ou {
                request = request.ou(optional_string(strand, &value, "ou")?);
            }
            // NETSETUP_MACHINE_PWD_PASSED requires a null account, so the two
            // credential forms cannot be combined. Rejecting the combination
            // here beats letting NetJoinDomain report a bare parameter error.
            let credentials = credentials(strand, account.as_deref(), password.as_deref())?;
            match (credentials, machine_password) {
                (Some(_), Some(_)) => {
                    return Err(Error::value(
                        strand,
                        "machine_password cannot be combined with account and password",
                    ));
                }
                (Some((account, password)), None) => {
                    request = request.credentials(account, password);
                }
                (None, Some(value)) => {
                    request = request.machine_password(string(strand, &value, "machine_password")?);
                }
                (None, None) => {}
            }
            options!(
                request,
                strand,
                create_account,
                join_if_joined,
                unsecure,
                defer_spn,
                force_spn,
                dc_account,
                with_new_name,
                readonly,
                ambiguous_dc,
                no_netlogon_cache,
                no_account_reuse,
            );
            domain::join(&dolang_ext_shell::vfs(strand), request)
                .await
                .into_sys(strand)?;
            Output::set(strand, out, Nil);
            Ok(())
        })
        .function("unjoin_domain", async move |strand, args, out| {
            let account_sym = global.syms.account;
            let password_sym = global.syms.password;
            let delete_account_sym = global.syms.delete_account;
            let ([], [account, password, delete_account]) = unpack!(
                strand,
                args,
                0,
                0,
                account_sym = None,
                password_sym = None,
                delete_account_sym = None
            )?;
            let mut request = domain::Unjoin::default();
            if let Some((account, password)) =
                credentials(strand, account.as_deref(), password.as_deref())?
            {
                request = request.credentials(account, password);
            }
            if let Some(value) = delete_account {
                request = request.delete_account(flag(strand, &value, "delete_account")?);
            }
            domain::unjoin(&dolang_ext_shell::vfs(strand), request)
                .await
                .into_sys(strand)?;
            Output::set(strand, out, Nil);
            Ok(())
        })
        .function("rename_machine", async move |strand, args, out| {
            let account_sym = global.syms.account;
            let password_sym = global.syms.password;
            let create_account_sym = global.syms.create_account;
            let ([name], [account, password, create_account]) = unpack!(
                strand,
                args,
                1,
                0,
                account_sym = None,
                password_sym = None,
                create_account_sym = None
            )?;
            let mut request = domain::Rename::new(string(strand, &name, "name")?);
            if let Some((account, password)) =
                credentials(strand, account.as_deref(), password.as_deref())?
            {
                request = request.credentials(account, password);
            }
            if let Some(value) = create_account {
                request = request.create_account(flag(strand, &value, "create_account")?);
            }
            domain::rename(&dolang_ext_shell::vfs(strand), request)
                .await
                .into_sys(strand)?;
            Output::set(strand, out, Nil);
            Ok(())
        })
        .function("provision_computer", async move |strand, args, out| {
            let machine_sym = global.syms.machine;
            let ou_sym = global.syms.ou;
            let dc_sym = global.syms.dc;
            let reuse_sym = global.syms.reuse;
            let default_password_sym = global.syms.default_password;
            let skip_account_search_sym = global.syms.skip_account_search;
            let root_ca_certs_sym = global.syms.root_ca_certs;
            let downlevel_priv_support_sym = global.syms.downlevel_priv_support;
            let (
                [domain_name, machine],
                [
                    ou,
                    dc,
                    reuse,
                    default_password,
                    skip_account_search,
                    root_ca_certs,
                    downlevel_priv_support,
                ],
            ) = unpack!(
                strand,
                args,
                1,
                0,
                machine_sym,
                ou_sym = None,
                dc_sym = None,
                reuse_sym = None,
                default_password_sym = None,
                skip_account_search_sym = None,
                root_ca_certs_sym = None,
                downlevel_priv_support_sym = None
            )?;
            let mut request = domain::Provision::new(
                string(strand, &domain_name, "domain")?,
                string(strand, &machine, "machine")?,
            );
            if let Some(value) = ou {
                request = request.ou(optional_string(strand, &value, "ou")?);
            }
            if let Some(value) = dc {
                request = request.dc(optional_string(strand, &value, "dc")?);
            }
            options!(
                request,
                strand,
                reuse,
                default_password,
                skip_account_search,
                root_ca_certs,
                downlevel_priv_support,
            );
            let blob = domain::provision(&dolang_ext_shell::vfs(strand), request)
                .await
                .into_sys(strand)?;
            Output::set(strand, out, blob.as_slice());
            Ok(())
        })
        .function("apply_offline_join", async move |strand, args, out| {
            let windows_path_sym = global.syms.windows_path;
            let online_sym = global.syms.online;
            let ([blob, path], [online]) =
                unpack!(strand, args, 1, 0, windows_path_sym, online_sym = None)?;
            let bytes = blob
                .as_bin(strand)
                .ok_or_else(|| Error::type_error(strand, "blob must be a Bin"))?
                .to_vec();
            let path = as_windows_path(strand, &path).ok_or_else(|| {
                Error::type_error(strand, "windows_path must be an fs.windows.Path")
            })?;
            let mut request = domain::OfflineJoin::new(bytes, path);
            if let Some(value) = online {
                request = request.online(flag(strand, &value, "online")?);
            }
            domain::apply_offline(&dolang_ext_shell::vfs(strand), request)
                .await
                .into_sys(strand)?;
            Output::set(strand, out, Nil);
            Ok(())
        })
}
