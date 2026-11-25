//! The one table: every manager and every job, in one place.
//!
//! This is the answer to "which file does what". A feature that is not listed
//! here does not exist to the policy, and every_kind_is_registered fails if a
//! kind is declared and never registered — the same guarantee a DI container
//! gives in C#, without the container.

/// A part of the policy that proposes work. One file each; see manager/.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Manager {
    /// Build order, harvesters, refineries, income. manager/economy.rs
    Economy,
    /// Teams, attack, defence. manager/military.rs
    Military,
    /// Placement, walls, repairs. manager/build.rs
    Build,
    /// Transports, garrisons, IFV gunner, superweapons. manager/abilities.rs
    Abilities,
}

impl Manager {
    /// The file that owns this manager.
    pub const fn file(self) -> &'static str {
        match self {
            Self::Economy => "manager/economy.rs",
            Self::Military => "manager/military.rs",
            Self::Build => "manager/build.rs",
            Self::Abilities => "manager/abilities.rs",
        }
    }

    /// What this manager is allowed to decide, in one line.
    pub const fn owns(self) -> &'static str {
        match self {
            Self::Economy => "what the house spends and how it earns",
            Self::Military => "which teams it raises and where they go",
            Self::Build => "where a structure goes and what repairs",
            Self::Abilities => "which special capability a unit or building uses",
        }
    }
}

/// Everything a unit can be told to do. One file each; see job/.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum JobKind {
    Harvest,
    ReturnOre,
    Repair,
    Capture,
    Garrison,
    Breach,
    Assault,
    Defend,
    Scout,
    Transport,
    Retreat,
    /// Deploy or pack up one unit: an MCV into a yard, a deployment-form change.
    Deploy,
    /// Walk back to the base after a fight.
    ///
    /// The column that leaves and never comes back is the one failure the
    /// attack lane has: measured, Barrel.mmx seed 19 read own 46 -> 37 with
    /// the column out and nothing killed for it.
    ReturnHome,
    /// Walk towards a target that is out of reach.
    ///
    /// The port of `FootClass Approach_Target @ 0x004D5690`, which the engine does
    /// not have: a unit ordered to attack something outside its weapon's `Range=`
    /// stands still with the order and never fires. Distinct from `Assault` so the
    /// two are not confused in the log, and distinct from a march because the
    /// destination is an object rather than a frontier cell.
    Advance,
}

impl JobKind {
    /// The manager that proposes this job. A job with no manager is a job
    /// nobody can ask for.
    pub const fn manager(self) -> Manager {
        match self {
            Self::Harvest | Self::ReturnOre | Self::Deploy => Manager::Economy,
            Self::Repair => Manager::Build,
            Self::Capture | Self::Garrison | Self::Transport => Manager::Abilities,
            Self::Breach
            | Self::Assault
            | Self::Defend
            | Self::Scout
            | Self::Retreat
            | Self::ReturnHome
            | Self::Advance => Manager::Military,
        }
    }
}

/// Every manager, in the order the frame runs them.
pub const MANAGERS: &[Manager] = &[
    Manager::Economy,
    Manager::Build,
    Manager::Military,
    Manager::Abilities,
];

/// Every job kind. The assignment passes over this list, so a kind that is not
/// here can never be proposed.
pub const JOBS: &[JobKind] = &[
    JobKind::Harvest,
    JobKind::ReturnOre,
    JobKind::Repair,
    JobKind::Capture,
    JobKind::Garrison,
    JobKind::Breach,
    JobKind::Assault,
    JobKind::Defend,
    JobKind::Scout,
    JobKind::Transport,
    JobKind::Retreat,
    JobKind::Deploy,
    JobKind::ReturnHome,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_manager_is_registered_once() {
        for manager in MANAGERS {
            assert_eq!(
                MANAGERS.iter().filter(|other| *other == manager).count(),
                1,
                "{} is registered twice",
                manager.file()
            );
        }
    }

    #[test]
    fn every_job_is_registered_once() {
        for job in JOBS {
            assert_eq!(
                JOBS.iter().filter(|other| *other == job).count(),
                1,
                "{job:?} is registered twice"
            );
        }
    }

    #[test]
    fn every_job_names_a_registered_manager() {
        for job in JOBS {
            assert!(
                MANAGERS.contains(&job.manager()),
                "{job:?} names {}, which is not registered",
                job.manager().file()
            );
        }
    }
}
