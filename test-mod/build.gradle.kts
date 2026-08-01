plugins {
    java
}

dependencies {
    compileOnly(project(":host"))
    compileOnly("org.quiltmc:quilt-loader:0.30.0")
}

java {
    toolchain {
        languageVersion = JavaLanguageVersion.of(25)
    }
}
