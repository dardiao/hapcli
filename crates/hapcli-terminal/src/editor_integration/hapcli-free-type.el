;;; hapcli-free-type.el --- hapcli terminal editor integration -*- lexical-binding: t; -*-

;; This adapter reports explicit Emacs/Evil editing state and installs bounded
;; operation keys used only after hapcli receives a current heartbeat.

(defvar hapcli-free-type--last-state nil)
(defvar hapcli-free-type--heartbeat nil)

(defun hapcli-free-type--private-osc-available-p ()
  "Return non-nil only when private OSC is isolated to an hapcli session."
  (let ((term-program (downcase (or (getenv "TERM_PROGRAM") "")))
        (lc-terminal (downcase (or (getenv "LC_TERMINAL") ""))))
    (and (= (length (or (getenv "TMUX") "")) 0)
         (= (length (or (getenv "STY") "")) 0)
         (= (length (or (getenv "ZELLIJ") "")) 0)
         (or (equal (getenv "hapcli_PRIVATE_OSC") "1")
             (equal term-program "hapcli")
             (equal lc-terminal "hapcli")))))

(defun hapcli-free-type--write-terminal (payload)
  "Write PAYLOAD only across an isolated hapcli terminal boundary."
  (when (hapcli-free-type--private-osc-available-p)
    (send-string-to-terminal payload)))

(defun hapcli-free-type--mode-name ()
  (if (bound-and-true-p evil-local-mode)
      (pcase evil-state
        ('normal "normal")
        ('insert "insert")
        ('replace "replace")
        ('visual "visual")
        (_ "emacs"))
    "emacs"))

(defun hapcli-free-type--selection-name ()
  (if (use-region-p) "region" "none"))

(defun hapcli-free-type--state-payload (active)
  (format "\e]7719;v=3;kind=editor-state;app=emacs;mode=%s;selection=%s;caps=mouse,clipboard,edit;active=%d\a"
          (hapcli-free-type--mode-name)
          (hapcli-free-type--selection-name)
          (if active 1 0)))

(defun hapcli-free-type--emit-state (&optional force)
  (let ((payload (hapcli-free-type--state-payload t)))
    (when (or force (not (equal payload hapcli-free-type--last-state)))
      (setq hapcli-free-type--last-state payload)
      (hapcli-free-type--write-terminal payload))))

(defun hapcli-free-type--percent-encode (text)
  ;; Encode every UTF-8 byte so the OSC field grammar never depends on URI
  ;; libraries choosing which printable characters to preserve.
  (mapconcat (lambda (byte) (format "%%%02X" byte))
             (string-to-list (encode-coding-string text 'utf-8))
             ""))

(defun hapcli-free-type--emit-clipboard (operation text)
  (hapcli-free-type--write-terminal
   (format "\e]7719;v=3;kind=editor-clipboard;app=emacs;op=%s;data=%s\a"
           operation
           (hapcli-free-type--percent-encode text))))

(defun hapcli-free-type-copy-selection ()
  (interactive)
  (when (use-region-p)
    (let ((text (buffer-substring-no-properties
                 (region-beginning) (region-end))))
      (kill-new text)
      (hapcli-free-type--emit-clipboard "copy" text)))
  (hapcli-free-type--emit-state t))

(defun hapcli-free-type-cut-selection ()
  (interactive)
  (when (use-region-p)
    (let ((text (buffer-substring-no-properties
                 (region-beginning) (region-end))))
      (kill-region (region-beginning) (region-end))
      (hapcli-free-type--emit-clipboard "cut" text)))
  (hapcli-free-type--emit-state t))

(defun hapcli-free-type-prepare-paste ()
  (interactive)
  (when (use-region-p)
    (delete-region (region-beginning) (region-end)))
  (hapcli-free-type--emit-state t))

(defun hapcli-free-type-delete-selection ()
  (interactive)
  (when (use-region-p)
    (delete-region (region-beginning) (region-end)))
  (hapcli-free-type--emit-state t))

(defvar hapcli-free-type-mode-map
  (let ((map (make-sparse-keymap)))
    (define-key map [hapcli-free-type-copy]
                #'hapcli-free-type-copy-selection)
    (define-key map [hapcli-free-type-cut]
                #'hapcli-free-type-cut-selection)
    (define-key map [hapcli-free-type-paste]
                #'hapcli-free-type-prepare-paste)
    (define-key map [hapcli-free-type-delete]
                #'hapcli-free-type-delete-selection)
    map))

;;;###autoload
(define-minor-mode hapcli-free-type-mode
  "Expose explicit terminal editing state to hapcli Free Type Mode."
  :global t
  :keymap hapcli-free-type-mode-map
  (if hapcli-free-type-mode
      (progn
        (define-key input-decode-map "\e[99;5~" [hapcli-free-type-copy])
        (define-key input-decode-map "\e[99;6~" [hapcli-free-type-cut])
        (define-key input-decode-map "\e[99;7~" [hapcli-free-type-paste])
        (define-key input-decode-map "\e[99;8~" [hapcli-free-type-delete])
        (when (fboundp 'xterm-mouse-mode)
          (xterm-mouse-mode 1))
        (add-hook 'post-command-hook #'hapcli-free-type--emit-state)
        (setq hapcli-free-type--heartbeat
              (run-at-time 0 1 #'hapcli-free-type--emit-state t)))
    (remove-hook 'post-command-hook #'hapcli-free-type--emit-state)
    (when (timerp hapcli-free-type--heartbeat)
      (cancel-timer hapcli-free-type--heartbeat))
    (setq hapcli-free-type--heartbeat nil)
    (hapcli-free-type--write-terminal (hapcli-free-type--state-payload nil))))

(add-hook 'kill-emacs-hook
          (lambda ()
            (when hapcli-free-type-mode
              (hapcli-free-type--write-terminal
               (hapcli-free-type--state-payload nil)))))

(provide 'hapcli-free-type)
;;; hapcli-free-type.el ends here
