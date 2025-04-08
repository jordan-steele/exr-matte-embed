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
                    if (releaseExists && !params.OVERWRITE_EXISTING) {
                        error "Release ${releaseVersion} already exists and OVERWRITE_EXISTING is not selected. Aborting."
                    } else if (releaseExists && params.OVERWRITE_EXISTING) {
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
                                
                                # Install dependencies
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
                        
                        stage('Create macOS Intel App Bundle') {
                            steps {
                                sh '''
                                # Activate the virtual environment
                                . venv/bin/activate
                                
                                # Set deployment target
                                export MACOSX_DEPLOYMENT_TARGET="12.0"
                                
                                # Create app bundle
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
                        
                        stage('Create DMG (Intel)') {
                            steps {
                                sh """
                                # Create DMG with version number
                                create-dmg \\
                                  --volname "EXR Matte Embed ${APP_VERSION}" \\
                                  --window-pos 200 120 \\
                                  --window-size 800 400 \\
                                  --icon-size 100 \\
                                  --icon "EXR Matte Embed.app" 200 190 \\
                                  --hide-extension "EXR Matte Embed.app" \\
                                  --app-drop-link 600 185 \\
                                  "dist/EXR-Matte-Embed_${APP_VERSION}_macos-intel.dmg" \\
                                  "dist/EXR Matte Embed.app"
                                """
                            }
                        }
                        
                        stage('Archive macOS Intel Artifact') {
                            steps {
                                stash includes: "dist/EXR-Matte-Embed_${APP_VERSION}_macos-intel.dmg", name: 'macos-intel-dmg'
                            }
                        }
                        
                        stage('Upload macOS Intel to Cloudflare R2') {
                            steps {
                                script {
                                    def artifactFile = "dist/EXR-Matte-Embed_${APP_VERSION}_macos-intel.dmg"
                                    def s3Path = "exr-matte-embed/releases/v${APP_VERSION}/EXR-Matte-Embed_${APP_VERSION}_macos-intel.dmg"
                                    
                                    uploadToR2(artifactFile, s3Path, R2_BUCKET_NAME)
                                    
                                    // Store the R2 URL for later use
                                    env.MACOS_INTEL_R2_URL = "${env.R2_PUBLIC_DOMAIN}/${s3Path}"
                                    echo "Uploaded macOS Intel build to R2: ${env.MACOS_INTEL_R2_URL}"
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
                                
                                # Install dependencies
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
                        
                        stage('Create macOS ARM App Bundle') {
                            steps {
                                sh '''
                                # Activate the virtual environment
                                . venv/bin/activate
                                
                                # Set deployment target
                                export MACOSX_DEPLOYMENT_TARGET="12.0"
                                
                                # Create app bundle
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
                        
                        stage('Create DMG (ARM)') {
                            steps {
                                sh """
                                # Create DMG with version number
                                create-dmg \\
                                  --volname "EXR Matte Embed ${APP_VERSION}" \\
                                  --window-pos 200 120 \\
                                  --window-size 800 400 \\
                                  --icon-size 100 \\
                                  --icon "EXR Matte Embed.app" 200 190 \\
                                  --hide-extension "EXR Matte Embed.app" \\
                                  --app-drop-link 600 185 \\
                                  "dist/EXR-Matte-Embed_${APP_VERSION}_macos-apple-silicon.dmg" \\
                                  "dist/EXR Matte Embed.app"
                                """
                            }
                        }
                        
                        stage('Archive macOS ARM Artifact') {
                            steps {
                                stash includes: "dist/EXR-Matte-Embed_${APP_VERSION}_macos-apple-silicon.dmg", name: 'macos-arm-dmg'
                            }
                        }
                        
                        stage('Upload macOS ARM to Cloudflare R2') {
                            steps {
                                script {
                                    def artifactFile = "dist/EXR-Matte-Embed_${APP_VERSION}_macos-apple-silicon.dmg"
                                    def s3Path = "exr-matte-embed/releases/v${APP_VERSION}/EXR-Matte-Embed_${APP_VERSION}_macos-apple-silicon.dmg"
                                    
                                    uploadToR2(artifactFile, s3Path, R2_BUCKET_NAME)
                                    
                                    // Store the R2 URL for later use
                                    env.MACOS_ARM_R2_URL = "${env.R2_PUBLIC_DOMAIN}/${s3Path}"
                                    echo "Uploaded macOS ARM build to R2: ${env.MACOS_ARM_R2_URL}"
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
                                
                                REM Install dependencies
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
                        
                        stage('Create Windows Executable') {
                            steps {
                                bat '''
                                @echo off
                                REM Activate virtual environment
                                call venv\\Scripts\\activate.bat
                                
                                REM Create Windows executable
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
                        
                        stage('Rename Windows Executable') {
                            steps {
                                bat """
                                @echo off
                                move "dist\\EXR Matte Embed.exe" "dist\\EXR-Matte-Embed_${APP_VERSION}_windows.exe"
                                """
                            }
                        }
                        
                        stage('Archive Windows Artifact') {
                            steps {
                                stash includes: "dist/EXR-Matte-Embed_${APP_VERSION}_windows.exe", name: 'windows-installer'
                            }
                        }
                        
                        stage('Upload Windows to Cloudflare R2') {
                            steps {
                                script {
                                    def artifactFile = "dist/EXR-Matte-Embed_${APP_VERSION}_windows.exe"
                                    def s3Path = "exr-matte-embed/releases/v${APP_VERSION}/EXR-Matte-Embed_${APP_VERSION}_windows.exe"
                                    
                                    uploadToR2(artifactFile, s3Path, R2_BUCKET_NAME)
                                    
                                    // Store the R2 URL for later use
                                    env.WINDOWS_R2_URL = "${env.R2_PUBLIC_DOMAIN}/${s3Path}"
                                    echo "Uploaded Windows build to R2: ${env.WINDOWS_R2_URL}"
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
                    
                    // Unstash artifacts from all platforms
                    unstash 'macos-intel-dmg'
                    unstash 'macos-arm-dmg'
                    unstash 'windows-installer'
                    
                    // Copy artifacts to the root directory (without 'dist/' prefix)
                    sh """
                        cp "dist/EXR-Matte-Embed_${env.APP_VERSION}_macos-intel.dmg" .
                        cp "dist/EXR-Matte-Embed_${env.APP_VERSION}_macos-apple-silicon.dmg" .
                        cp "dist/EXR-Matte-Embed_${env.APP_VERSION}_windows.exe" .
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
                        
                        // Upload all assets to the release
                        uploadGithubReleaseAsset(
                            credentialId: 'github-creds',
                            repository: 'jordan-steele/exr-matte-embed',
                            tagName: releaseVersion,
                            uploadAssets: [
                                [filePath: "EXR-Matte-Embed_${env.APP_VERSION}_macos-intel.dmg", name: "EXR-Matte-Embed_${env.APP_VERSION}_macos-intel.dmg"],
                                [filePath: "EXR-Matte-Embed_${env.APP_VERSION}_macos-apple-silicon.dmg", name: "EXR-Matte-Embed_${env.APP_VERSION}_macos-apple-silicon.dmg"],
                                [filePath: "EXR-Matte-Embed_${env.APP_VERSION}_windows.exe", name: "EXR-Matte-Embed_${env.APP_VERSION}_windows.exe"]
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