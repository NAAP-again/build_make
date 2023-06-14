/*
 * Copyright (C) 2023 The Android Open Source Project
 *
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 *      http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */

use anyhow::Result;
use serde::Serialize;
use std::path::PathBuf;
use tinytemplate::TinyTemplate;

use crate::aconfig::{FlagState, Permission};
use crate::cache::{Cache, Item};
use crate::codegen;
use crate::commands::OutputFile;

pub fn generate_java_code(cache: &Cache) -> Result<Vec<OutputFile>> {
    let package = cache.package();
    let class_elements: Vec<ClassElement> =
        cache.iter().map(|item| create_class_element(package, item)).collect();
    let is_read_write = class_elements.iter().any(|item| item.is_read_write);
    let context = Context { package_name: package.to_string(), is_read_write, class_elements };

    let java_files = vec!["Flags.java", "FeatureFlagsImpl.java", "FeatureFlags.java"];

    let mut template = TinyTemplate::new();
    template.add_template("Flags.java", include_str!("../templates/Flags.java.template"))?;
    template.add_template(
        "FeatureFlagsImpl.java",
        include_str!("../templates/FeatureFlagsImpl.java.template"),
    )?;
    template.add_template(
        "FeatureFlags.java",
        include_str!("../templates/FeatureFlags.java.template"),
    )?;

    let path: PathBuf = package.split('.').collect();
    java_files
        .iter()
        .map(|file| {
            Ok(OutputFile {
                contents: template.render(file, &context)?.into(),
                path: path.join(file),
            })
        })
        .collect::<Result<Vec<OutputFile>>>()
}

#[derive(Serialize)]
struct Context {
    pub package_name: String,
    pub is_read_write: bool,
    pub class_elements: Vec<ClassElement>,
}

#[derive(Serialize)]
struct ClassElement {
    pub default_value: String,
    pub device_config_namespace: String,
    pub device_config_flag: String,
    pub flag_name_constant_suffix: String,
    pub is_read_write: bool,
    pub method_name: String,
}

fn create_class_element(package: &str, item: &Item) -> ClassElement {
    let device_config_flag = codegen::create_device_config_ident(package, &item.name)
        .expect("values checked at cache creation time");
    ClassElement {
        default_value: if item.state == FlagState::Enabled {
            "true".to_string()
        } else {
            "false".to_string()
        },
        device_config_namespace: item.namespace.clone(),
        device_config_flag,
        flag_name_constant_suffix: item.name.to_ascii_uppercase(),
        is_read_write: item.permission == Permission::ReadWrite,
        method_name: item.name.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_generate_java_code() {
        let cache = crate::test::create_cache();
        let generated_files = generate_java_code(&cache).unwrap();
        let expect_flags_content = r#"
        package com.android.aconfig.test;
        public final class Flags {
            public static boolean disabled_ro() {
                return FEATURE_FLAGS.disabled_ro();
            }
            public static boolean disabled_rw() {
                return FEATURE_FLAGS.disabled_rw();
            }
            public static boolean enabled_ro() {
                return FEATURE_FLAGS.enabled_ro();
            }
            public static boolean enabled_rw() {
                return FEATURE_FLAGS.enabled_rw();
            }
            private static FeatureFlags FEATURE_FLAGS = new FeatureFlagsImpl();

        }
        "#;
        let expected_featureflagsimpl_content = r#"
        package com.android.aconfig.test;
        import android.provider.DeviceConfig;
        public final class FeatureFlagsImpl implements FeatureFlags {
            @Override
            public boolean disabled_ro() {
                return false;
            }
            @Override
            public boolean disabled_rw() {
                return DeviceConfig.getBoolean(
                    "aconfig_test",
                    "com.android.aconfig.test.disabled_rw",
                    false
                );
            }
            @Override
            public boolean enabled_ro() {
                return true;
            }
            @Override
            public boolean enabled_rw() {
                return DeviceConfig.getBoolean(
                    "aconfig_test",
                    "com.android.aconfig.test.enabled_rw",
                    true
                );
            }
        }
        "#;
        let expected_featureflags_content = r#"
        package com.android.aconfig.test;
        public interface FeatureFlags {
            boolean disabled_ro();
            boolean disabled_rw();
            boolean enabled_ro();
            boolean enabled_rw();
        }
        "#;
        let mut file_set = HashMap::from([
            ("com/android/aconfig/test/Flags.java", expect_flags_content),
            ("com/android/aconfig/test/FeatureFlagsImpl.java", expected_featureflagsimpl_content),
            ("com/android/aconfig/test/FeatureFlags.java", expected_featureflags_content),
        ]);

        for file in generated_files {
            let file_path = file.path.to_str().unwrap();
            assert!(file_set.contains_key(file_path), "Cannot find {}", file_path);
            assert_eq!(
                None,
                crate::test::first_significant_code_diff(
                    file_set.get(file_path).unwrap(),
                    &String::from_utf8(file.contents.clone()).unwrap()
                ),
                "File {} content is not correct",
                file_path
            );
            file_set.remove(file_path);
        }

        assert!(file_set.is_empty());
    }
}
