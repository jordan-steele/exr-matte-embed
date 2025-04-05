pipeline {
    agent {
        label 'default'
    }
    
    parameters {
        booleanParam(name: 'CREATE_RELEASE', defaultValue: false, description: 'Create a release')
        text(name: 'RELEASE_NOTES', defaultValue: 'Release notes go here', description: 'Notes for this release')
    }
    
    environment {
        R2_ACCOUNT_ID = credentials('r2-account-id')
        R2_BUCKET_NAME = credentials('r2-public-bucket')
        R2_PUBLIC_DOMAIN = credentials('r2-public-domain')
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
        
        stage('Build All Platforms') {
            parallel {
                stage('Build macOS') {
                    agent {
                        label 'mini-mac-pro'  // Use Mac agent for macOS build
                    }
                    stages {
                        stage('Setup Python Environment (macOS)') {
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
                        
                        stage('Install Dependencies (macOS)') {
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
                        
                        stage('Display Package Info (macOS)') {
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
                        
                        stage('Install create-dmg (macOS)') {
                            steps {
                                sh 'brew install create-dmg || echo "create-dmg is already installed"'
                            }
                        }
                        
                        stage('Create macOS App Bundle') {
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
                        
                        stage('Create DMG') {
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
                        
                        stage('Archive macOS Artifact') {
                            steps {
                                stash includes: "dist/EXR-Matte-Embed_${APP_VERSION}_macos-intel.dmg", name: 'macos-dmg'
                            }
                        }
                        
                        stage('Upload macOS to Cloudflare R2') {
                            steps {
                                script {
                                    def artifactFile = "dist/EXR-Matte-Embed_${APP_VERSION}_macos-intel.dmg"
                                    def s3Path = "exr-matte-embed/releases/v${APP_VERSION}/EXR-Matte-Embed_${APP_VERSION}_macos-intel.dmg"
                                    
                                    uploadToR2(artifactFile, s3Path)
                                    
                                    // Store the R2 URL for later use
                                    env.MACOS_R2_URL = "${env.R2_PUBLIC_DOMAIN}/${s3Path}"
                                    echo "Uploaded macOS build to R2: ${env.MACOS_R2_URL}"
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
                                    
                                    uploadToR2(artifactFile, s3Path)
                                    
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
                    
                    // Unstash artifacts from both platforms
                    unstash 'macos-dmg'
                    unstash 'windows-installer'
                    
                    // Just use the provided release notes without Cloudflare links
                    def notesWithLinks = """${params.RELEASE_NOTES}"""
                    
                    // Create a release notes file
                    writeFile file: 'release-notes.md', text: notesWithLinks
                    
                    // Create GitHub release with both assets
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
                        
                        // Upload both assets to the release
                        uploadGithubReleaseAsset(
                            credentialId: 'github-creds',
                            repository: 'jordan-steele/exr-matte-embed',
                            tagName: releaseVersion,
                            uploadAssets: [
                                [filePath: "dist/EXR-Matte-Embed_${env.APP_VERSION}_macos-intel.dmg", name: "EXR-Matte-Embed_${env.APP_VERSION}_macos-intel.dmg"],
                                [filePath: "dist/EXR-Matte-Embed_${env.APP_VERSION}_windows.exe", name: "EXR-Matte-Embed_${env.APP_VERSION}_windows.exe"]
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

// Helper function to upload artifacts to Cloudflare R2
def uploadToR2(String artifactFile, String s3Path) {
    withCredentials([
        string(credentialsId: 'r2-access-key', variable: 'R2_ACCESS_KEY'),
        string(credentialsId: 'r2-secret-key', variable: 'R2_SECRET_KEY')
    ]) {
        if (isUnix()) {
            // Unix system (macOS)
            sh 'which aws || pip install awscli'
            
            sh '''
                # Create AWS CLI profile for R2
                mkdir -p ~/.aws
                cat > ~/.aws/config << EOF
[profile r2]
region = auto
output = json
endpoint_url = https://${R2_ACCOUNT_ID}.r2.cloudflarestorage.com
EOF

                cat > ~/.aws/credentials << EOF
[r2]
aws_access_key_id = ${R2_ACCESS_KEY}
aws_secret_access_key = ${R2_SECRET_KEY}
EOF
            '''
            
            sh "aws --profile r2 s3 cp \"${artifactFile}\" s3://${env.R2_BUCKET_NAME}/${s3Path}"
        } else {
            // Windows system - simplified approach
            bat '''
                @echo off
                REM Activate virtual environment
                call venv\\Scripts\\activate.bat
                
                REM Install AWS CLI if not present
                pip install awscli
                
                REM Create AWS config directory
                if not exist "%USERPROFILE%\\.aws" mkdir "%USERPROFILE%\\.aws"
                
                REM Create AWS config files
                echo [profile r2] > "%USERPROFILE%\\.aws\\config"
                echo region = auto >> "%USERPROFILE%\\.aws\\config"
                echo output = json >> "%USERPROFILE%\\.aws\\config"
                echo endpoint_url = https://%R2_ACCOUNT_ID%.r2.cloudflarestorage.com >> "%USERPROFILE%\\.aws\\config"
                
                echo [r2] > "%USERPROFILE%\\.aws\\credentials"
                echo aws_access_key_id = %R2_ACCESS_KEY% >> "%USERPROFILE%\\.aws\\credentials"
                echo aws_secret_access_key = %R2_SECRET_KEY% >> "%USERPROFILE%\\.aws\\credentials"
            '''
            
            // Execute AWS CLI through Python
            bat """
                @echo off
                call venv\\Scripts\\activate.bat
                python -m awscli --profile r2 s3 cp "${artifactFile}" s3://%R2_BUCKET_NAME%/${s3Path}
            """
        }
    }
}