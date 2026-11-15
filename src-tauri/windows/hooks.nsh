; Deskmate uninstall cleanup (GitHub issue #2: a full uninstall left settings,
; keys and registry entries behind, and a reinstall picked them up again).
;
; An update must touch none of this. The in-app updater starts the installer
; with /UPDATE, which copies the new files over the old ones and does not run
; the uninstaller at all, so these hooks do not run. If an installer ever runs
; the uninstaller in update mode, $UpdateMode is 1 there and both hooks skip.
; Start with Windows, the settings, the saved keys, the CPU temperature helper,
; the family protection and the Deskmate service therefore survive. Keep every
; step added here behind the same $UpdateMode check.

!macro NSIS_HOOK_PREUNINSTALL
  ; The Deskmate service (service.rs in the app) is removed with Deskmate,
  ; whether or not the application data goes too: it runs the bundled
  ; $INSTDIR\service\deskmate-service.exe uninstall, which needs one
  ; administrator approval. Without an installed service it asks for nothing.
  ; Runs while that file still exists.
  ${If} $UpdateMode <> 1
    ExecWait '"$INSTDIR\${MAINBINARYNAME}.exe" --remove-service'
  ${EndIf}

  ; "Delete the application data" ticked and not an update: settings, logs and
  ; saved keys live outside the install folder, and only the app knows which
  ; Credential Manager entries are its own. Runs while deskmate.exe still exists.
  ${If} $DeleteAppDataCheckboxState = 1
  ${AndIf} $UpdateMode <> 1
    ExecWait '"$INSTDIR\${MAINBINARYNAME}.exe" --forget-this-computer'
  ${EndIf}
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  ; Registered by the app at every start; after an uninstall they would point at
  ; a deleted program. An update re-creates them on the next start anyway.
  ${If} $UpdateMode <> 1
    DeleteRegKey HKCU "Software\Classes\deskmate"
    DeleteRegKey HKCU "Software\Classes\AppUserModelId\com.deskmate.desktop"
    DeleteRegKey HKCU "Software\Classes\CLSID\{9C4F1E7A-3B62-4D18-9F0C-2E5A7B6D4C31}"
  ${EndIf}
!macroend
