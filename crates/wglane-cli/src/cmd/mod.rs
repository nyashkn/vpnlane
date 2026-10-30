//! One module per subcommand. Each defines `Args` (with its own help text)
//! and `run(&Context, &Args)`.

pub(crate) mod completions;
pub(crate) mod doctor;
pub(crate) mod down;
pub(crate) mod gen_conf;
pub(crate) mod install_helper;
pub(crate) mod servers;
pub(crate) mod status;
pub(crate) mod tui;
pub(crate) mod up;
pub(crate) mod watch;

/// `up` -> `status` -> `down` through the real command functions, with fakes
/// at the port boundary and a real config dir.
#[cfg(test)]
mod e2e {
    use std::net::IpAddr;

    use wglane_core::TailnetHealth;
    use wglane_core::testing::{
        FakeBypassProvider, FakeCatalog, FakeExitIp, FakeRoutes, FakeTailnet, FakeToken, FakeTunnel,
    };
    use wglane_core::{PublicKey, Server};

    use super::{down, status, up};
    use crate::context::Context;

    #[test]
    fn up_status_down() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = Context {
            json: false,
            verbose: 0,
            config_dir: dir.path().to_owned(),
        };
        let za = "ZA".parse().unwrap();
        let catalog = FakeCatalog {
            servers: [(
                za,
                vec![Server {
                    hostname: "za3.nordvpn.com".into(),
                    station: "192.0.2.2".parse().unwrap(),
                    load: 9,
                    public_key: PublicKey("pk".into()),
                    country: za,
                }],
            )]
            .into(),
            private_key: Some("priv".into()),
            ..Default::default()
        };
        let tunnel = FakeTunnel::default();
        let routes = FakeRoutes::new("192.0.2.254".parse::<IpAddr>().unwrap());
        let bypass = FakeBypassProvider(
            ["198.51.100.1/32", "198.51.100.2/32"]
                .iter()
                .map(|c| c.parse().unwrap())
                .collect(),
        );
        let store = ctx.conf_store();
        let status = || {
            status::gather(
                &tunnel,
                &FakeExitIp("192.0.2.7".parse().unwrap()),
                &FakeTailnet(TailnetHealth::NotInstalled),
                ctx.recorded_routes(),
            )
            .0
        };

        let done = up::connect_recorded(
            &ctx,
            &tunnel,
            &routes,
            &[&bypass],
            &catalog,
            &FakeToken(Some("tok".into())),
            za,
        )
        .unwrap();
        assert!(
            done.summary(za)
                .ends_with("config regenerated, 2 bypass routes")
        );
        assert!(store.path(za).exists());
        let s = status();
        assert_eq!((s.active, s.bypass_routes), (vec!["za".to_owned()], 2));

        let gone = down::disconnect_recorded(&ctx, &tunnel).unwrap();
        assert_eq!(gone.summary(), "disconnected");
        assert_eq!(ctx.recorded_routes(), 0, "down clears the route record");
        let s = status();
        assert_eq!((s.active, s.bypass_routes), (vec![], 0));
    }
}
