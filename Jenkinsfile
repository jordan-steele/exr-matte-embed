// Rust beta artifacts only. Public signing and release publication are separate.
// Provision Rust 1.95+ (including rustfmt and clippy) on the existing agents.
pipeline {
    agent none
    options {
        buildDiscarder(logRotator(numToKeepStr: '5'))
        disableConcurrentBuilds()
    }
    stages {
        stage('Build native platforms') {
            parallel {
                stage('macOS Intel') {
                    agent { label 'mini-mac-pro' }
                    steps {
                        checkout scm
                        sh 'cargo fmt --all --check'
                        sh 'cargo test --locked --all-targets'
                        sh 'cargo clippy --locked --all-targets --all-features -- -D warnings'
                        sh 'scripts/package_macos.sh'
                        archiveArtifacts artifacts: 'target/EXR-Matte-Embed-macos-*.zip,target/exr-matte-embed-cli-macos-*', fingerprint: true
                    }
                }
                stage('macOS Apple Silicon') {
                    agent { label 'mac-studio' }
                    steps {
                        checkout scm
                        sh 'cargo fmt --all --check'
                        sh 'cargo test --locked --all-targets'
                        sh 'cargo clippy --locked --all-targets --all-features -- -D warnings'
                        sh 'scripts/package_macos.sh'
                        archiveArtifacts artifacts: 'target/EXR-Matte-Embed-macos-*.zip,target/exr-matte-embed-cli-macos-*', fingerprint: true
                    }
                }
                stage('Windows') {
                    agent { label 'windows-11' }
                    steps {
                        checkout scm
                        bat 'cargo fmt --all --check'
                        bat 'cargo test --locked --all-targets'
                        bat 'cargo clippy --locked --all-targets --all-features -- -D warnings'
                        bat 'cargo build --release --locked --bin exr-matte-embed --bin exr-matte-embed-cli'
                        archiveArtifacts artifacts: 'target/release/exr-matte-embed.exe,target/release/exr-matte-embed-cli.exe', fingerprint: true
                    }
                }
            }
        }
    }
}
