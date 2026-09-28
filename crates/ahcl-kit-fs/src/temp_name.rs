// crates/ahcl-kit-fs/src/temp_name.rs - Exclusive temporary component names.
//
// Copyright (C) 2026 Aperip Daedalus Foundation. All rights reserved.
//
// This file is part of AHCL Kit and is provided under version 1.1 of the
// Aperip Heimdall Commons License (AHCL). The applicable version is also subject
// to the AHCL provisions concerning Continuous AHCL Licensing Segments and
// migration to later official versions.
//
// After having a reasonable opportunity to read AHCL, all applicable Additional
// Restrictions, and all version notices, a person accepts the corresponding terms,
// to the extent permitted by applicable law, by using, copying, modifying, building,
// using this file as a dependency, deploying, distributing, or operating this file
// over a network.
//
// Official AHCL text and public notices:          https://ahcl.aperip.com
// AHCL Materials Directory:                       .ahcl/
// Repository official or recognized AHCL copy:   .ahcl/AHCL-1.1.md
// Project canonical repository:                   https://github.com/Aperip-Daedalus-Foundation/ahcl-kit
// AHCL origin and project notice:                 .ahcl/AHCL-PROJECT-NOTICE.md
// AHCL Version Adoption records:                  .ahcl/AHCL-VERSION-ADOPTION.md
// Complete Corresponding Source and history:      .ahcl/AHCL-SOURCE.md
// Dependencies, Referenced Materials, and licenses:
//                                                    .ahcl/AHCL-DEPENDENCIES.md
//
// SPDX-License-Identifier: LicenseRef-AHCL-1.1

use std::ffi::OsString;

pub(crate) fn temporary_component() -> Result<OsString, ()> {
    temporary_component_from(crate::fill_random)
}

fn temporary_component_from<E>(
    fill: impl FnOnce(&mut [u8]) -> Result<(), E>,
) -> Result<OsString, E> {
    let mut random = [0_u8; 16];
    fill(&mut random)?;
    Ok(encode_temp_name(&random))
}

fn encode_temp_name(random: &[u8; 16]) -> OsString {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut name = String::with_capacity(14 + random.len() * 2);
    name.push_str(".ahcl-kit-tmp-");
    for byte in random {
        name.push(char::from(HEX[usize::from(byte >> 4)]));
        name.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    OsString::from(name)
}

#[cfg(test)]
mod temp_component_contracts {
    use super::{encode_temp_name, temporary_component_from};
    use std::collections::BTreeSet;

    #[test]
    fn randomness_failure_stops_generation() {
        let error = temporary_component_from(|_| Err("rng unavailable"))
            .expect_err("randomness failure must stop temporary-name generation");

        assert_eq!(error, "rng unavailable");
    }

    #[test]
    fn every_random_byte_changes_the_temporary_name() {
        let mut names = BTreeSet::new();
        let zero_name = encode_temp_name(&[0_u8; 16]);
        assert!(names.insert(zero_name));

        for index in 0..16 {
            for value in 1..=u8::MAX {
                let mut random = [0_u8; 16];
                random[index] = value;
                let name = encode_temp_name(&random);
                let name_text = name.to_str().expect("temporary name is ASCII");
                assert!(name_text.starts_with(".ahcl-kit-tmp-"));
                assert_eq!(name_text.len(), 46);
                assert!(
                    names.insert(name),
                    "duplicate for byte {index} value {value}"
                );
            }
        }
        assert_eq!(names.len(), 4_081);
    }
}
