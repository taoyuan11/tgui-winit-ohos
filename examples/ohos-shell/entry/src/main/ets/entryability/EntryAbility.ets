import { AbilityConstant, UIAbility, Want } from '@kit.AbilityKit';
import { hilog } from '@kit.PerformanceAnalysisKit';
import { window } from '@kit.ArkUI';

const DOMAIN = 0x3433;

export default class EntryAbility extends UIAbility {
  onCreate(want: Want, launchParam: AbilityConstant.LaunchParam): void {
    hilog.info(DOMAIN, 'tgui-winit-ohos', 'EntryAbility onCreate');
  }

  onWindowStageCreate(windowStage: window.WindowStage): void {
    try {
      const mainWindow = windowStage.getMainWindowSync();
      mainWindow.setWindowLayoutFullScreen(true)
        .then(() => {
          hilog.info(DOMAIN, 'tgui-winit-ohos', 'Enabled immersive window layout');
          return mainWindow.setWindowSystemBarEnable([]);
        })
        .then(() => {
          hilog.info(DOMAIN, 'tgui-winit-ohos', 'Disabled status and navigation bars');
        })
        .catch((err: Error) => {
          hilog.error(DOMAIN, 'tgui-winit-ohos', 'Failed to configure immersive window: %{public}s', JSON.stringify(err));
        });
    } catch (err) {
      hilog.error(DOMAIN, 'tgui-winit-ohos', 'Failed to get main window: %{public}s', JSON.stringify(err));
    }

    windowStage.loadContent('pages/Index', (err) => {
      if (err.code) {
        hilog.error(DOMAIN, 'tgui-winit-ohos', 'Failed to load page: %{public}s', JSON.stringify(err));
      }
    });
  }
}
