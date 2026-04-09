import bridge from 'libentry.so';

@Entry
@Component
struct Index {
  private debugTimer?: number;

  aboutToAppear(): void {
    this.debugTimer = setInterval(() => {
      console.info(
        `[tgui-winit-ohos] message=${bridge.getMessage()} events=${bridge.getEventCount()} redraws=${bridge.getRedrawCount()}`
      );
    }, 1000);
  }

  aboutToDisappear(): void {
    if (this.debugTimer !== undefined) {
      clearInterval(this.debugTimer);
      this.debugTimer = undefined;
    }
  }

  build() {
    Stack() {
      XComponent({
        id: 'winit-surface',
        type: 'surface',
        libraryname: 'entry'
      })
        .width('100%')
        .height('100%')
        .focusable(true)
        .expandSafeArea([SafeAreaType.SYSTEM], [SafeAreaEdge.TOP, SafeAreaEdge.BOTTOM])
    }
    .width('100%')
    .height('100%')
    .expandSafeArea([SafeAreaType.SYSTEM], [SafeAreaEdge.TOP, SafeAreaEdge.BOTTOM])
  }
}
