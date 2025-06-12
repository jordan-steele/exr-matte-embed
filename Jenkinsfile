pipeline {
    agent {
        label 'default'
    }
    
    parameters {
        booleanParam(name: 'CREATE_RELEASE', defaultValue: false, description: 'Create a release')
        booleanParam(name: 'OVERWRITE_EXISTING', defaultValue: false, description: 'Overwrite release if version already exists')
        text(name: 'RELEASE_NOTES', defaultValue: 'Release notes go here', description: 'Notes for this release')
    }
    
    environment {
        R2_ACCOUNT_ID = credentials('r2-account-id')
        R2_BUCKET_NAME = credentials('r2-public-bucket')
        R2_PUBLIC_DOMAIN = credentials('r2-public-domain')
        GITHUB_TOKEN = credentials('github-creds')
    }
    
    options {
        buildDiscarder(logRotator(numToKeepStr: '5'))
        disableConcurrentBuilds()
    }
    
    stages {
        stage('Checkout') {
            steps {
                checkout scm
            }
        }
        
        stage('Get Version') {
            steps {
                script {
                    // Read version from version.py file
                    def versionContent = readFile('version.py').trim()
                    def versionMatch = versionContent =~ /VERSION\s*=\s*"([^"]*)"/ 
                    if (versionMatch) {
                        env.APP_VERSION = versionMatch[0][1]
                        echo "Detected version: ${env.APP_VERSION}"
                    } else {
                        error "Could not parse version from version.py"
                    }
                }
            }
        }

        stage('Check If Release Exists') {
            steps {
                script {
                    def releaseExists = false
                    def releaseVersion = "v${env.APP_VERSION}"
                    
                    // Check if the tag/release already exists on GitHub
                    try {
                        def response = sh(
                            script: """
                                curl -s -o /dev/null -w "%{http_code}" -H "Authorization: token ${GITHUB_TOKEN}" \
                                https://api.github.com/repos/jordan-steele/exr-matte-embed/releases/tags/${releaseVersion}
                            """,
                            returnStdout: true
                        ).trim()
                        
                        // If we get a 200 response, the release exists
                        if (response == "200") {
                            releaseExists = true
                            echo "Release ${releaseVersion} already exists on GitHub!"
                        } else {
                            echo "Release ${releaseVersion} does not exist on GitHub."
                        }
                    } catch (Exception e) {
                        // If there's an error, assume the release doesn't exist
                        echo "Error checking release: ${e.message}"
                        echo "Assuming release doesn't exist."
                    }
                    
                    // Store the result for later stages
                    env.RELEASE_EXISTS = releaseExists.toString()
                    
                    // If release exists and overwrite is not checked, abort the build
                    if (releaseExists && !params.OVERWRITE_EXISTING && params.CREATE_RELEASE) {
                        error "Release ${releaseVersion} already exists and OVERWRITE_EXISTING is not selected. Aborting."
                    } else if (releaseExists && params.OVERWRITE_EXISTING && params.CREATE_RELEASE) {
                        echo "Release ${releaseVersion} exists, but will be overwritten as requested."
                        
                        // Delete the existing release if we're going to overwrite it
                        sh """
                            # Get the release ID
                            RELEASE_ID=\$(curl -s -H "Authorization: token ${GITHUB_TOKEN}" \
                                https://api.github.com/repos/jordan-steele/exr-matte-embed/releases/tags/${releaseVersion} | \
                                grep -o '\"id\": [0-9]*' | head -1 | awk '{print \$2}')
                            
                            # Delete the release using its ID
                            curl -X DELETE -H "Authorization: token ${GITHUB_TOKEN}" \
                                https://api.github.com/repos/jordan-steele/exr-matte-embed/releases/\$RELEASE_ID
                            
                            # Delete the tag - attempt remotely first
                            git fetch
                            git push https://\${GITHUB_TOKEN}@github.com/jordan-steele/exr-matte-embed.git --delete ${releaseVersion} || echo "Remote tag delete attempt completed"
                            
                            # Also try to delete local tag if it exists
                            git tag -d ${releaseVersion} || echo "No local tag to delete"
                        """
                    } else if (releaseExists && !params.CREATE_RELEASE) {
                        echo "Release ${releaseVersion} exists, but we're not creating a release in this build, so continuing."
                    }
                }
            }
        }
        
        stage('Build All Platforms') {
            parallel {
                stage('Build macOS Intel') {
                    agent {
                        label 'mini-mac-pro'  // Use Intel Mac agent for Intel build
                    }
                    stages {
                        stage('Setup Python Environment (macOS Intel)') {
                            steps {
                                sh '''
                                # Create a virtual environment if it doesn't exist
                                python3 -m venv venv
                                
                                # Activate the virtual environment
                                . venv/bin/activate
                                
                                # Upgrade pip in the virtual environment
                                python -m pip install --upgrade pip
                                '''
                            }
                        }
                        
                        stage('Install Dependencies (macOS Intel)') {
                            steps {
                                sh '''
                                # Activate the virtual environment
                                . venv/bin/activate
                                
                                # Install GUI dependencies for GUI build
                                python -m pip install -r requirements.txt
                                python -m pip install pyinstaller
                                '''
                            }
                        }
                        
                        stage('Display Package Info (macOS Intel)') {
                            steps {
                                sh '''
                                # Activate the virtual environment
                                . venv/bin/activate
                                
                                # Display info
                                python -m pip list
                                python -c "import numpy; print(numpy.__file__)"
                                python -c "import OpenEXR; print(OpenEXR.__file__)"
                                '''
                            }
                        }
                        
                        stage('Install create-dmg (macOS Intel)') {
                            steps {
                                sh 'brew install create-dmg || echo "create-dmg is already installed"'
                            }
                        }
                        
                        stage('Create macOS Intel GUI App Bundle') {
                            steps {
                                sh '''
                                # Activate the virtual environment
                                . venv/bin/activate
                                
                                # Set deployment target
                                export MACOSX_DEPLOYMENT_TARGET="12.0"
                                
                                # Create GUI app bundle
                                python -m PyInstaller \\
                                  --windowed \\
                                  --name "EXR Matte Embed" \\
                                  --icon "images/icon.icns" \\
                                  --add-data "images:images" \\
                                  --hidden-import numpy \\
                                  --copy-metadata OpenEXR \\
                                  --copy-metadata numpy \\
                                  main.py
                                '''
                            }
                        }
                        
                        stage('Create macOS Intel CLI Binary') {
                            steps {
                                sh '''
                                # Activate the virtual environment
                                . venv/bin/activate
                                
                                # Set deployment target
                                export MACOSX_DEPLOYMENT_TARGET="12.0"
                                
                                # Create CLI binary using the spec file
                                python -m PyInstaller "EXR Matte Embed CLI.spec"
                                
                                # Rename CLI binary to include version and platform
                                mv "dist/exr-matte-embed-cli" "dist/exr-matte-embed-cli_${APP_VERSION}_macos-intel"
                                '''
                            }
                        }
                        
                        stage('Create DMG (Intel)') {
                            steps {
                                sh """
                                # Create DMG with version number for GUI
                                create-dmg \\
                                  --volname "EXR Matte Embed ${APP_VERSION}" \\
                                  --window-pos 200 120 \\
                                  --window-size 800 400 \\
                                  --icon-size 100 \\
                                  --icon "EXR Matte Embed.app" 200 190 \\
                                  --hide-extension "EXR Matte Embed.app" \\
                                  --app-drop-link 600 185 \\
                                  "dist/EXR-Matte-Embed-GUI_${APP_VERSION}_macos-intel.dmg" \\
                                  "dist/EXR Matte Embed.app"
                                """
                            }
                        }
                        
                        stage('Archive macOS Intel Artifacts') {
                            steps {
                                stash includes: "dist/EXR-Matte-Embed-GUI_${APP_VERSION}_macos-intel.dmg", name: 'macos-intel-gui-dmg'
                                stash includes: "dist/exr-matte-embed-cli_${APP_VERSION}_macos-intel", name: 'macos-intel-cli'
                            }
                        }
                        
                        stage('Upload macOS Intel to Cloudflare R2') {
                            steps {
                                script {
                                    // Upload GUI DMG
                                    def guiArtifactFile = "dist/EXR-Matte-Embed-GUI_${APP_VERSION}_macos-intel.dmg"
                                    def guiS3Path = "exr-matte-embed/releases/v${APP_VERSION}/EXR-Matte-Embed-GUI_${APP_VERSION}_macos-intel.dmg"
                                    uploadToR2(guiArtifactFile, guiS3Path, R2_BUCKET_NAME)
                                    env.MACOS_INTEL_GUI_R2_URL = "${env.R2_PUBLIC_DOMAIN}/${guiS3Path}"
                                    
                                    // Upload CLI binary
                                    def cliArtifactFile = "dist/exr-matte-embed-cli_${APP_VERSION}_macos-intel"
                                    def cliS3Path = "exr-matte-embed/releases/v${APP_VERSION}/exr-matte-embed-cli_${APP_VERSION}_macos-intel"
                                    uploadToR2(cliArtifactFile, cliS3Path, R2_BUCKET_NAME)
                                    env.MACOS_INTEL_CLI_R2_URL = "${env.R2_PUBLIC_DOMAIN}/${cliS3Path}"
                                    
                                    echo "Uploaded macOS Intel GUI to R2: ${env.MACOS_INTEL_GUI_R2_URL}"
                                    echo "Uploaded macOS Intel CLI to R2: ${env.MACOS_INTEL_CLI_R2_URL}"
                                }
                            }
                        }
                    }
                    post {
                        always {
                            cleanWs()
                        }
                    }
                }
                
                stage('Build macOS Apple Silicon') {
                    agent {
                        label 'mac-studio'  // Use Apple Silicon agent for ARM build
                    }
                    stages {
                        stage('Setup Python Environment (macOS ARM)') {
                            steps {
                                sh '''
                                # Create a virtual environment if it doesn't exist
                                python3 -m venv venv
                                
                                # Activate the virtual environment
                                . venv/bin/activate
                                
                                # Upgrade pip in the virtual environment
                                python -m pip install --upgrade pip
                                '''
                            }
                        }
                        
                        stage('Install Dependencies (macOS ARM)') {
                            steps {
                                sh '''
                                # Activate the virtual environment
                                . venv/bin/activate
                                
                                # Install GUI dependencies for GUI build
                                python -m pip install -r requirements.txt
                                python -m pip install pyinstaller
                                '''
                            }
                        }
                        
                        stage('Display Package Info (macOS ARM)') {
                            steps {
                                sh '''
                                # Activate the virtual environment
                                . venv/bin/activate
                                
                                # Display info about the environment
                                python -m pip list
                                python -c "import numpy; print(numpy.__file__)"
                                python -c "import OpenEXR; print(OpenEXR.__file__)"
                                python -c "import platform; print('Architecture:', platform.machine())"
                                '''
                            }
                        }
                        
                        stage('Install create-dmg (macOS ARM)') {
                            steps {
                                sh 'brew install create-dmg || echo "create-dmg is already installed"'
                            }
                        }
                        
                        stage('Create macOS ARM GUI App Bundle') {
                            steps {
                                sh '''
                                # Activate the virtual environment
                                . venv/bin/activate
                                
                                # Set deployment target
                                export MACOSX_DEPLOYMENT_TARGET="12.0"
                                
                                # Create GUI app bundle
                                python -m PyInstaller \\
                                  --windowed \\
                                  --name "EXR Matte Embed" \\
                                  --icon "images/icon.icns" \\
                                  --add-data "images:images" \\
                                  --hidden-import numpy \\
                                  --copy-metadata OpenEXR \\
                                  --copy-metadata numpy \\
                                  main.py
                                '''
                            }
                        }
                        
                        stage('Create macOS ARM CLI Binary') {
                            steps {
                                sh '''
                                # Activate the virtual environment
                                . venv/bin/activate
                                
                                # Set deployment target
                                export MACOSX_DEPLOYMENT_TARGET="12.0"
                                
                                # Create CLI binary using the spec file
                                python -m PyInstaller "EXR Matte Embed CLI.spec"
                                
                                # Rename CLI binary to include version and platform
                                mv "dist/exr-matte-embed-cli" "dist/exr-matte-embed-cli_${APP_VERSION}_macos-apple-silicon"
                                '''
                            }
                        }
                        
                        stage('Create DMG (ARM)') {
                            steps {
                                sh """
                                # Create DMG with version number for GUI
                                create-dmg \\
                                  --volname "EXR Matte Embed ${APP_VERSION}" \\
                                  --window-pos 200 120 \\
                                  --window-size 800 400 \\
                                  --icon-size 100 \\
                                  --icon "EXR Matte Embed.app" 200 190 \\
                                  --hide-extension "EXR Matte Embed.app" \\
                                  --app-drop-link 600 185 \\
                                  "dist/EXR-Matte-Embed-GUI_${APP_VERSION}_macos-apple-silicon.dmg" \\
                                  "dist/EXR Matte Embed.app"
                                """
                            }
                        }
                        
                        stage('Archive macOS ARM Artifacts') {
                            steps {
                                stash includes: "dist/EXR-Matte-Embed-GUI_${APP_VERSION}_macos-apple-silicon.dmg", name: 'macos-arm-gui-dmg'
                                stash includes: "dist/exr-matte-embed-cli_${APP_VERSION}_macos-apple-silicon", name: 'macos-arm-cli'
                            }
                        }
                        
                        stage('Upload macOS ARM to Cloudflare R2') {
                            steps {
                                script {
                                    // Upload GUI DMG
                                    def guiArtifactFile = "dist/EXR-Matte-Embed-GUI_${APP_VERSION}_macos-apple-silicon.dmg"
                                    def guiS3Path = "exr-matte-embed/releases/v${APP_VERSION}/EXR-Matte-Embed-GUI_${APP_VERSION}_macos-apple-silicon.dmg"
                                    uploadToR2(guiArtifactFile, guiS3Path, R2_BUCKET_NAME)
                                    env.MACOS_ARM_GUI_R2_URL = "${env.R2_PUBLIC_DOMAIN}/${guiS3Path}"
                                    
                                    // Upload CLI binary
                                    def cliArtifactFile = "dist/exr-matte-embed-cli_${APP_VERSION}_macos-apple-silicon"
                                    def cliS3Path = "exr-matte-embed/releases/v${APP_VERSION}/exr-matte-embed-cli_${APP_VERSION}_macos-apple-silicon"
                                    uploadToR2(cliArtifactFile, cliS3Path, R2_BUCKET_NAME)
                                    env.MACOS_ARM_CLI_R2_URL = "${env.R2_PUBLIC_DOMAIN}/${cliS3Path}"
                                    
                                    echo "Uploaded macOS ARM GUI to R2: ${env.MACOS_ARM_GUI_R2_URL}"
                                    echo "Uploaded macOS ARM CLI to R2: ${env.MACOS_ARM_CLI_R2_URL}"
                                }
                            }
                        }
                    }
                    post {
                        always {
                            cleanWs()
                        }
                    }
                }
                
                stage('Build Windows') {
                    agent {
                        label 'windows-11'  // Use Windows agent for Windows build
                    }
                    stages {
                        stage('Setup Python Environment (Windows)') {
                            steps {
                                bat '''
                                @echo off
                                REM Create virtual environment
                                python -m venv venv
                                
                                REM Activate virtual environment
                                call venv\\Scripts\\activate.bat
                                
                                REM Upgrade pip
                                python -m pip install --upgrade pip
                                '''
                            }
                        }
                        
                        stage('Install Dependencies (Windows)') {
                            steps {
                                bat '''
                                @echo off
                                REM Activate virtual environment
                                call venv\\Scripts\\activate.bat
                                
                                REM Install GUI dependencies for GUI build
                                python -m pip install -r requirements.txt
                                python -m pip install pyinstaller
                                '''
                            }
                        }
                        
                        stage('Display Package Info (Windows)') {
                            steps {
                                bat '''
                                @echo off
                                REM Activate virtual environment
                                call venv\\Scripts\\activate.bat
                                
                                REM Display info
                                pip list
                                python -c "import numpy; print(numpy.__file__)"
                                python -c "import OpenEXR; print(OpenEXR.__file__)"
                                '''
                            }
                        }
                        
                        stage('Create Windows GUI Executable') {
                            steps {
                                bat '''
                                @echo off
                                REM Activate virtual environment
                                call venv\\Scripts\\activate.bat
                                
                                REM Create Windows GUI executable
                                python -m PyInstaller ^
                                  --onefile ^
                                  --windowed ^
                                  --name "EXR Matte Embed" ^
                                  --icon "images\\icon.ico" ^
                                  --add-data "images\\icon.ico;images/" ^
                                  --hidden-import numpy ^
                                  --copy-metadata OpenEXR ^
                                  --copy-metadata numpy ^
                                  main.py
                                '''
                            }
                        }
                        
                        stage('Create Windows CLI Executable') {
                            steps {
                                bat '''
                                @echo off
                                REM Activate virtual environment
                                call venv\\Scripts\\activate.bat
                                
                                REM Create Windows CLI executable using spec file
                                python -m PyInstaller "EXR Matte Embed CLI.spec"
                                
                                REM Rename CLI executable to include version and platform
                                move "dist\\exr-matte-embed-cli.exe" "dist\\exr-matte-embed-cli_%APP_VERSION%_windows.exe"
                                '''
                            }
                        }
                        
                        stage('Rename Windows GUI Executable') {
                            steps {
                                bat """
                                @echo off
                                move "dist\\EXR Matte Embed.exe" "dist\\EXR-Matte-Embed-GUI_${APP_VERSION}_windows.exe"
                                """
                            }
                        }
                        
                        stage('Archive Windows Artifacts') {
                            steps {
                                stash includes: "dist/EXR-Matte-Embed-GUI_${APP_VERSION}_windows.exe", name: 'windows-gui-installer'
                                stash includes: "dist/exr-matte-embed-cli_${APP_VERSION}_windows.exe", name: 'windows-cli'
                            }
                        }
                        
                        stage('Upload Windows to Cloudflare R2') {
                            steps {
                                script {
                                    // Upload GUI executable
                                    def guiArtifactFile = "dist/EXR-Matte-Embed-GUI_${APP_VERSION}_windows.exe"
                                    def guiS3Path = "exr-matte-embed/releases/v${APP_VERSION}/EXR-Matte-Embed-GUI_${APP_VERSION}_windows.exe"
                                    uploadToR2(guiArtifactFile, guiS3Path, R2_BUCKET_NAME)
                                    env.WINDOWS_GUI_R2_URL = "${env.R2_PUBLIC_DOMAIN}/${guiS3Path}"
                                    
                                    // Upload CLI executable
                                    def cliArtifactFile = "dist/exr-matte-embed-cli_${APP_VERSION}_windows.exe"
                                    def cliS3Path = "exr-matte-embed/releases/v${APP_VERSION}/exr-matte-embed-cli_${APP_VERSION}_windows.exe"
                                    uploadToR2(cliArtifactFile, cliS3Path, R2_BUCKET_NAME)
                                    env.WINDOWS_CLI_R2_URL = "${env.R2_PUBLIC_DOMAIN}/${cliS3Path}"
                                    
                                    echo "Uploaded Windows GUI to R2: ${env.WINDOWS_GUI_R2_URL}"
                                    echo "Uploaded Windows CLI to R2: ${env.WINDOWS_CLI_R2_URL}"
                                }
                            }
                        }
                    }
                    post {
                        always {
                            cleanWs()
                        }
                    }
                }
            }
        }

        stage('Update R2 Releases JSON') {
            steps {
                build job: 'Update R2 Releases', parameters: [
                    string(name: 'BUCKET_ID', value: "${env.R2_BUCKET_NAME}"),
                    string(name: 'BASE_PATH', value: 'exr-matte-embed')
                ]
            }
        }
        
        stage('Create GitHub Release') {
            when {
                expression { return params.CREATE_RELEASE == true }
            }
            steps {
                script {
                    def releaseVersion = "v${env.APP_VERSION}"
                    
                    // Create directories for unstashed artifacts
                    sh "mkdir -p dist"
                    
                    // Unstash GUI artifacts from all platforms
                    unstash 'macos-intel-gui-dmg'
                    unstash 'macos-arm-gui-dmg'
                    unstash 'windows-gui-installer'
                    
                    // Unstash CLI artifacts from all platforms
                    unstash 'macos-intel-cli'
                    unstash 'macos-arm-cli'
                    unstash 'windows-cli'
                    
                    // Copy artifacts to the root directory (without 'dist/' prefix)
                    sh """
                        # GUI artifacts
                        cp "dist/EXR-Matte-Embed-GUI_${env.APP_VERSION}_macos-intel.dmg" .
                        cp "dist/EXR-Matte-Embed-GUI_${env.APP_VERSION}_macos-apple-silicon.dmg" .
                        cp "dist/EXR-Matte-Embed-GUI_${env.APP_VERSION}_windows.exe" .
                        
                        # CLI artifacts
                        cp "dist/exr-matte-embed-cli_${env.APP_VERSION}_macos-intel" .
                        cp "dist/exr-matte-embed-cli_${env.APP_VERSION}_macos-apple-silicon" .
                        cp "dist/exr-matte-embed-cli_${env.APP_VERSION}_windows.exe" .
                    """
                    
                    // Just use the provided release notes without Cloudflare links
                    def notesWithLinks = """${params.RELEASE_NOTES}"""
                    
                    // Create a release notes file
                    writeFile file: 'release-notes.md', text: notesWithLinks
                    
                    // Create GitHub release with all assets
                    withCredentials([string(credentialsId: 'github-creds', variable: 'GITHUB_TOKEN')]) {
                        // Create GitHub release
                        createGitHubRelease(
                            credentialId: 'github-creds',
                            repository: 'jordan-steele/exr-matte-embed',
                            tag: releaseVersion,
                            commitish: 'main',
                            bodyFile: 'release-notes.md',
                            draft: false
                        )
                        
                        // Upload all GUI and CLI assets to the release
                        uploadGithubReleaseAsset(
                            credentialId: 'github-creds',
                            repository: 'jordan-steele/exr-matte-embed',
                            tagName: releaseVersion,
                            uploadAssets: [
                                // GUI Assets
                                [filePath: "EXR-Matte-Embed-GUI_${env.APP_VERSION}_macos-intel.dmg", name: "EXR-Matte-Embed-GUI_${env.APP_VERSION}_macos-intel.dmg"],
                                [filePath: "EXR-Matte-Embed-GUI_${env.APP_VERSION}_macos-apple-silicon.dmg", name: "EXR-Matte-Embed-GUI_${env.APP_VERSION}_macos-apple-silicon.dmg"],
                                [filePath: "EXR-Matte-Embed-GUI_${env.APP_VERSION}_windows.exe", name: "EXR-Matte-Embed-GUI_${env.APP_VERSION}_windows.exe"],
                                // CLI Assets
                                [filePath: "exr-matte-embed-cli_${env.APP_VERSION}_macos-intel", name: "exr-matte-embed-cli_${env.APP_VERSION}_macos-intel"],
                                [filePath: "exr-matte-embed-cli_${env.APP_VERSION}_macos-apple-silicon", name: "exr-matte-embed-cli_${env.APP_VERSION}_macos-apple-silicon"],
                                [filePath: "exr-matte-embed-cli_${env.APP_VERSION}_windows.exe", name: "exr-matte-embed-cli_${env.APP_VERSION}_windows.exe"]
                            ]
                        )
                    }
                }
            }
        }
    }
    
    post {
        always {
            cleanWs()
        }
    }
}