use crate::devel::{load_devel_info, save_devel_info};
use crate::print_error;
use crate::search::interactive_search_local;
use crate::util::pkg_base_or_name;
use crate::Config;
use crate::{exec, repo};

use std::collections::{HashMap, HashSet};

use anyhow::Result;

fn find_optdep_packages(config: &Config, removal_list: &[String]) -> HashSet<String> {
    let removal_set: HashSet<&str> = removal_list.iter().map(|s| s.as_str()).collect();
    let target_set: HashSet<&str> = config.targets.iter().map(|s| s.as_str()).collect();
    let db = config.alpm.localdb();

    let mut kept_as_optdeps = HashSet::new();

    for pkg in db.pkgs() {
        if removal_set.contains(pkg.name()) {
            continue;
        }

        for optdep in pkg.optdepends() {
            let dep_name = optdep.name();
            if removal_set.contains(dep_name) && !target_set.contains(dep_name) {
                kept_as_optdeps.insert(dep_name.to_string());
            }
        }
    }

    kept_as_optdeps
}

pub fn remove(config: &mut Config) -> Result<i32> {
    if config.interactive {
        interactive_search_local(config)?;
    }

    let mut devel_info = load_devel_info(config)?.unwrap_or_default();
    let db = config.alpm.localdb();
    let bases = config
        .targets
        .iter()
        .filter_map(|pkg| db.pkg(pkg.as_str()).ok())
        .map(pkg_base_or_name)
        .collect::<Vec<_>>();

    let mut db_map: HashMap<String, Vec<String>> = HashMap::new();
    let (_, local_repos) = repo::repo_aur_dbs(config);
    for pkg in &config.targets {
        for db in &local_repos {
            if let Ok(pkg) = db.pkg(pkg.as_str()) {
                db_map
                    .entry(db.name().to_string())
                    .or_default()
                    .push(pkg.name().to_string());
            }
        }
    }

    let is_recursive = config.args.has_arg("s", "recursive");

    let modified_args;
    let args = if config.keep_optdeps && is_recursive {
        let mut print_args = config.pacman_args();
        print_args.arg("print");
        print_args.push_value("print-format", "%n");
        let output = exec::pacman_output(config, &print_args)?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let removal_list: Vec<String> = stdout
            .lines()
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .collect();

        let kept = find_optdep_packages(config, &removal_list);

        if kept.is_empty() {
            &config.args
        } else {
            let c = config.color;
            for pkg in &kept {
                println!(
                    "{} {} is an optional dependency of another package -- skipping removal",
                    c.warning.paint("::"),
                    c.bold.paint(pkg.as_str()),
                );
            }

            let filtered: Vec<String> = removal_list
                .into_iter()
                .filter(|p| !kept.contains(p.as_str()))
                .collect();

            modified_args = {
                let mut args = config.args.clone();
                args.remove("s").remove("recursive");
                args.targets = filtered;
                args
            };
            &modified_args
        }
    } else {
        &config.args
    };

    let mut ret = exec::pacman(config, args)?.code();
    if ret != 0 {
        return Ok(ret);
    }

    let (_, dbs) = repo::repo_aur_dbs(config);

    for target in bases {
        devel_info.info.remove(target);
    }

    drop(dbs);

    if let Err(err) = save_devel_info(config, &devel_info) {
        print_error(config.color.error, err);
        ret = 1;
    }

    Ok(ret)
}
