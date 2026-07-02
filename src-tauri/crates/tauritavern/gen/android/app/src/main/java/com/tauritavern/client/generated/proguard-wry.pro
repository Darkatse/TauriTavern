# THIS FILE IS AUTO-GENERATED. DO NOT MODIFY!!

# Copyright 2020-2023 Tauri Programme within The Commons Conservancy
# SPDX-License-Identifier: Apache-2.0
# SPDX-License-Identifier: MIT

-keep class com.tauritavern.client.* {
  native <methods>;
}

-keep class com.tauritavern.client.WryActivity {
  public <init>(...);

  void setWebView(com.tauritavern.client.RustWebView);
  java.lang.Class getAppClass(...);
  java.lang.String getVersion();
}

-keep class com.tauritavern.client.Ipc {
  public <init>(...);

  @android.webkit.JavascriptInterface public <methods>;
}

-keep class com.tauritavern.client.RustWebView {
  public <init>(...);

  void loadUrlMainThread(...);
  void loadHTMLMainThread(...);
  void evalScript(...);
}

-keep class com.tauritavern.client.RustWebChromeClient,com.tauritavern.client.RustWebViewClient {
  public <init>(...);
}
