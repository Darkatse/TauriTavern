# Add project specific ProGuard rules here.
# You can control the set of applied configuration files using the
# proguardFiles setting in build.gradle.
#
# For more details, see
#   http://developer.android.com/guide/developing/tools/proguard.html

# If your project uses WebView with JS, uncomment the following
# and specify the fully qualified class name to the JavaScript interface
# class:
#-keepclassmembers class fqcn.of.javascript.interface.for.webview {
#   public *;
#}

# ML Kit (barcode scanner) instantiates these registrars through reflection.
# firebase-components 16.1.0 keeps the classes but not their constructors, which
# AGP 9 no longer keeps implicitly. Drop this once its consumer rules do.
-keepclassmembers class * implements com.google.firebase.components.ComponentRegistrar {
   public <init>();
}

# Uncomment this to preserve the line number information for
# debugging stack traces.
#-keepattributes SourceFile,LineNumberTable

# If you keep the line number information, uncomment this to
# hide the original source file name.
#-renamesourcefileattribute SourceFile
