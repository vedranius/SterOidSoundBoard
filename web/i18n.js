"use strict";
// Two languages for the whole UI: Croatian (hr) and English (en).
// The page is written in both (DigiLingua in Croatian, Music in English); this
// module translates every visible text — static HTML, text rendered by the
// scripts, placeholders, tooltips, options, dialogs, canvas labels and server
// messages — into the chosen language. The choice is kept per browser.
const I18N = (() => {
  let lang = "hr";
  try { lang = localStorage.getItem("ssb.lang") || ((navigator.language || "hr").toLowerCase().startsWith("hr") ? "hr" : "en"); } catch (_) {}
  if (lang !== "en") lang = "hr";

  // [Croatian, English]
  const PAIRS = [
    // ---------------------------------------------------------------- header, Music, audio panel
    ["Glazba", "Music"], ["Zvuk", "Audio"], ["Preseti", "Presets"], ["Utišaj", "Mute"], ["Utišano", "Muted"],
    ["Utišaj izlaz (mjerači i snimanje rade dalje)", "Silence the output (meters and recording keep working)"],
    ["Postavke AI servisa", "AI provider settings"], ["Svijetla / tamna tema", "Light / dark theme"], ["Jezik", "Language"],
    ["Dodirnite za dodavanje", "Tap to add"], ["Upravljački program", "Driver"], ["Ulaz", "Input"], ["Izlaz", "Output"], ["Međuspremnik", "Buffer"],
    ["bez ulaza", "no input"], ["Pokreni", "Start"], ["Zaustavi", "Stop"], ["Učitaj", "Load"], ["Spremi kao…", "Save as…"], ["Obriši", "Delete"],
    ["Predlošci:", "Templates:"], ["Preset", "Preset"], ["(još nema preseta)", "(no presets yet)"], ["zadano", "default"],
    ["zvuk zaustavljen", "audio stopped"], ["prekinuta veza — ponovno spajanje…", "disconnected — reconnecting…"], ["bez ulaza", "no input"],
    ["Naziv preseta:", "Preset name:"], ["UKLJ.", "ON"], ["pojaseva", "bands"],
    ["DigiLingua — klinički lanac", "DigiLingua — clinical chain"], ["Prazan board", "Empty board"], ["Ulaz izravno na izlaz", "Input straight to output"],
    ["Odabir mikrofona → 31-pojasni EQ → DAF → FAF → prekidač → maskirajući šum → lijevo/desno uho", "Mic select → 31-band EQ → DAF → FAF → interrupter → masking noise → L/R ears"],
    // node names, categories, parameters (engine)
    ["Pojačanje", "Gain"], ["Filtar / EQ", "Filter / EQ"], ["Distorzija", "Drive"], ["Kašnjenje (delay)", "Delay"], ["Tremolo", "Tremolo"],
    ["31-pojasni EQ", "31-band EQ"], ["DAF kašnjenje", "DAF Delay"], ["Pomak visine / FAF", "Pitch Shift / FAF"], ["Šum", "Noise"], ["Prekidač", "Interrupter"],
    ["Kanali / balans", "Channels / Balance"], ["Pomoćno", "Utility"], ["Filtar", "Filter"], ["Distorzija", "Distortion"], ["Vrijeme", "Time"],
    ["Modulacija", "Modulation"], ["Visina", "Pitch"], ["Generator", "Generator"],
    ["Frekvencija", "Frequency"], ["Način", "Mode"], ["Niskopropusni", "Low-pass"], ["Visokopropusni", "High-pass"], ["Pojasnopropusni", "Band-pass"], ["Vršni", "Peak"],
    ["Ton", "Tone"], ["Razina", "Level"], ["Miks", "Mix"], ["Povratna veza", "Feedback"], ["Brzina", "Rate"], ["Dubina", "Depth"], ["Izlaz", "Output"],
    ["Kašnjenje", "Delay"], ["Suhi", "Dry"], ["Odgođeni", "Delayed"], ["Pomak", "Shift"], ["Prozor", "Window"], ["Boja", "Color"],
    ["Bijeli", "White"], ["Ružičasti", "Pink"], ["Smeđi", "Brown"], ["Uskopojasni", "Narrow-band"], ["Središte pojasa", "Band centre"], ["Uši", "Ears"],
    ["Oba", "Both"], ["Lijevo", "Left"], ["Desno", "Right"], ["Period", "Period"], ["Pauza", "Gap"], ["Pretapanje", "Fade"], ["Izvor", "Source"],
    ["Stereo", "Stereo"], ["Lijevi → oba", "Left → both"], ["Desni → oba", "Right → both"], ["Mono zbroj", "Mono sum"], ["Vrijeme", "Time"],

    // ---------------------------------------------------------------- DigiLingua layout
    ["Klijenti", "Clients"], ["Novi", "New"], ["Novi klijent", "New client"], ["Traži ime, šifru, dijagnozu…", "Search name, code, diagnosis…"],
    ["Uredi", "Edit"], ["Odaberi klijenta", "Select a client"], ["odaberite klijenta", "select a client"], ["Real-time audio", "Real-time audio"],
    ["Analiza glasa", "Voice analysis"], ["Napredak", "Progress"], ["Tipkovnički prečaci (?)", "Keyboard shortcuts (?)"], ["Tipkovnički prečaci", "Keyboard shortcuts"],
    ["Postavke ustanove, analize i kalibracije", "Clinic, analysis and calibration settings"], ["Pokreni / zaustavi zvuk (razmaknica)", "Start / stop audio (space)"],
    ["▶ Pokreni audio", "▶ Start audio"], ["■ Zaustavi zvuk", "■ Stop audio"], ["Terapijska sesija i povijest", "Therapy session and history"],
    ["Slušna povratna veza: EQ, DAF, FAF, šum, diskontinuitet", "Auditory feedback: EQ, DAF, FAF, noise, discontinuity"],
    ["Spektrogram, visina, formanti i akustička analiza (Praat)", "Spectrogram, pitch, formants and acoustic analysis (Praat)"],
    ["Mjere kroz vrijeme i usporedba snimaka", "Measures over time and comparison of recordings"],
    ["Trenutni board nema DigiLingua lanac (mikrofon → EQ → DAF → FAF → diskontinuitet → šum → uši).", "The current board has no DigiLingua chain (mic → EQ → DAF → FAF → discontinuity → noise → ears)."],
    ["Učitaj DigiLingua lanac", "Load the DigiLingua chain"], ["Zamjenjuje trenutni board — spremite ga prije kao preset ako ga želite zadržati.", "Replaces the current board — save it as a preset first if you want to keep it."],
    ["Trenutni klinički preset", "Current clinical preset"],
    // devices + status
    ["Ulazni uređaj", "Input device"], ["Izlazni uređaj", "Output device"], ["Osvježi popis uređaja (D)", "Refresh the device list (D)"],
    ["manji = manja latencija", "smaller = lower latency"], ["Status zvuka", "Audio status"], ["Zaustavljeno", "Stopped"], ["Radi", "Running"], ["Greška", "Error"],
    ["Uzorkovanje", "Sample rate"], ["Procj. latencija", "Est. latency"], ["DSP opterećenje", "DSP load"], ["Prekidi (xruns)", "Dropouts (xruns)"],
    ["Popis uređaja osvježen", "Device list refreshed"], ["Zvuk nije pokrenut — kliknite ▶ Pokreni audio", "Audio is not running — click ▶ Start audio"],
    ["Ponovno pokretanje zvuka zaustavlja snimanje. Nastaviti?", "Restarting audio stops the recording. Continue?"], ["Zaustavljanje zvuka zaustavlja snimanje. Nastaviti?", "Stopping audio stops the recording. Continue?"],
    // live sonagram + VU + chain
    ["Spektrogram u stvarnom vremenu", "Real-time spectrogram"], ["Uživo: valni oblik i spektrogram", "Live: waveform and spectrogram"],
    ["mikrofon (suhi)", "microphone (dry)"], ["izlaz (obrađeni)", "output (processed)"], ["Raspon", "Range"], ["Prozor", "Window"], ["Dinamika", "Dynamics"],
    ["auto razina", "auto level"], ["Maks", "Max"], ["Boje", "Colours"], ["Prikaz", "View"], ["valni oblik", "waveform"], ["intenzitet", "intensity"],
    ["Zamrzni (P)", "Freeze (P)"], ["Spremi sliku", "Save image"], ["Povećaj", "Enlarge"], ["čekam signal…", "waiting for signal…"], ["sada", "now"],
    ["Širokopojasni (5 ms)", "Wideband (5 ms)"], ["Srednji (15 ms)", "Medium (15 ms)"], ["Uskopojasni (30 ms)", "Narrowband (30 ms)"],
    ["Praat (sivo, bijela podloga)", "Praat (grey on white)"], ["Sivo (crna podloga)", "Grey on black"], ["Plavo-žuto", "Blue-yellow"], ["Toplo (inferno)", "Warm (inferno)"],
    ["Viridis", "Viridis"], ["Plavo", "Blue"], ["ZAMRZNUTO", "FROZEN"], ["(uživo, orijentacijski)", "(live, indicative)"],
    ["Uživo: jedan okvir od 10 ms, za praćenje. Mjere za nalaz računa Praat-kompatibilna analiza snimke.", "Live: one 10 ms frame, for monitoring. Measures for the report come from the Praat-compatible analysis of the recording."],
    ["Razina (dBFS)", "Level (dBFS)"], ["Mik", "Mic"], ["L", "L"], ["D", "R"], ["Mikrofon (ulaz)", "Microphone (input)"], ["Lijevo uho (izlaz)", "Left ear (output)"], ["Desno uho (izlaz)", "Right ear (output)"],
    ["Crta = vršna razina (zadržava 2 s). 🔊 utišava samo to uho.", "Line = peak level (held 2 s). 🔊 mutes only that ear."],
    ["Signalni lanac", "Signal chain"], ["Klik na modul uključuje ili isključuje ga.", "Click a module to switch it on or off."],
    ["Mikrofon", "Microphone"], ["EQ", "EQ"], ["Diskont.", "Discont."], ["ravno", "flat"], ["pojasa", "bands"], ["nema", "none"], ["stereo", "stereo"], ["L → oba", "L → both"], ["D → oba", "R → both"], ["mono", "mono"],
    ["Uključi", "Switch on"], ["Isključi", "Switch off"], ["Uključi lijevo uho", "Unmute the left ear"], ["Utišaj lijevo uho", "Mute the left ear"], ["Uključi desno uho", "Unmute the right ear"], ["Utišaj desno uho", "Mute the right ear"],
    // EQ
    ["31-pojasni ISO ekvilizator", "31-band ISO equalizer"], ["Klizači", "Sliders"], ["Krivulja", "Curve"], ["Brze krivulje", "Quick curves"], ["Brze krivulje…", "Quick curves…"],
    ["Reset", "Reset"], ["Vrati sve pojaseve na 0 dB (R)", "Set all bands to 0 dB (R)"], ["Ravno", "Flat"], ["Govor", "Speech"], ["Telefon", "Telephone"], ["Bas", "Bass"], ["Visoki", "Treble"],
    ["Bas 20–125 Hz", "Bass 20–125 Hz"], ["Niski srednji 160–630 Hz", "Low-mid 160–630 Hz"], ["Srednji 800 Hz–3,15 kHz", "Mid 800 Hz–3.15 kHz"], ["Visoki 4–20 kHz", "High 4–20 kHz"],
    ["povucite preko stupaca za glatku krivulju · dvoklik = 0 dB · ↑↓ = ±0,5 dB", "drag across the bars for a smooth curve · double-click = 0 dB · ↑↓ = ±0.5 dB"], ["sve 0 dB", "all 0 dB"],
    // presets
    ["Klinički preseti", "Clinical presets"], ["Spremi kao novi", "Save as new"], ["tvornički", "factory"], ["Učitaj preset", "Load preset"], ["Obriši preset", "Delete preset"],
    ["Spremi klinički preset", "Save clinical preset"], ["Naziv *", "Name *"], ["Kategorija", "Category"], ["Opis", "Description"], ["npr. Mucanje, Glas, Sluh", "e.g. Stuttering, Voice, Hearing"],
    ["npr. DAF 120 ms za pacijente s blokovima; smanjivati kroz terapiju", "e.g. DAF 120 ms for patients with blocks; reduce during therapy"],
    ["Spremaju se sve postavke: EQ, DAF, FAF, šum, diskontinuitet, glasnoća po uhu i uključeni moduli.", "All settings are saved: EQ, DAF, FAF, noise, discontinuity, per-ear volume and active modules."],
    ["Spremi", "Save"], ["Odustani", "Cancel"], ["Preset spremljen", "Preset saved"], ["Učitajte DigiLingua lanac prije spremanja preseta", "Load the DigiLingua chain before saving a preset"],
    ["Preset zamjenjuje trenutni board DigiLingua lancem. Nastaviti?", "The preset replaces the current board with the DigiLingua chain. Continue?"], ["Zamijeniti trenutni board DigiLingua lancem?", "Replace the current board with the DigiLingua chain?"],
    ["Vlastiti", "Custom"], ["Tvornički (DigiLingua)", "Factory (DigiLingua)"], ["Terapijski predložak", "Therapy template"],
    ["Ravni odziv (Flat Response)", "Flat Response"], ["Sve frekvencije na 0 dB — neutralna polazna točka za testiranje.", "All frequencies at 0 dB — a neutral starting point for testing."],
    ["Pojačani basovi (Bass Boost)", "Bass Boost"], ["Pojačanje niskih frekvencija, blago stišavanje visokih.", "Boosts low frequencies, slightly lowers the highs."],
    ["Pojačani visoki tonovi (Treble Enhance)", "Treble Enhance"], ["Pojačanje visokih frekvencija za jasnoću i razumljivost.", "Boosts high frequencies for clarity and intelligibility."],
    ["Fokus na govor (Speech Focus)", "Speech Focus"], ["Naglašeno govorno područje (160 Hz – 4 kHz), stišani krajevi spektra.", "Emphasises the speech range (160 Hz – 4 kHz), lowers both ends of the spectrum."],
    ["Tečnost — DAF 75 ms", "Fluency — DAF 75 ms"], ["Kratka odgođena slušna povratna veza (bez suhog glasa). Polazna točka, prilagoditi pacijentu.", "Short delayed auditory feedback (no dry voice). A starting point — adapt to the patient."],
    ["Tečnost — DAF 75 ms + FAF −½ oktave", "Fluency — DAF 75 ms + FAF −½ octave"], ["Kombinirana DAF i frekvencijski promijenjena povratna veza.", "Combined DAF and frequency-altered feedback."],
    ["Usporavanje govora — DAF 200 ms", "Slower speech — DAF 200 ms"], ["Dulja odgoda za usporavanje tempa govora.", "A longer delay to slow the speaking rate."],
    ["Glasnoća — maskirajući šum (Lombardov efekt)", "Loudness — masking noise (Lombard effect)"], ["Ružičasti šum u oba uha potiče glasniji govor; razinu povećavati postupno.", "Pink noise in both ears encourages louder speech; raise the level gradually."],
    ["Diskontinuitet 1 s / 200 ms", "Discontinuity 1 s / 200 ms"], ["Periodično prekidanje povratne veze.", "Periodic interruption of the feedback."],
    // modules
    ["DAF — odgođena slušna povratna veza", "DAF — delayed auditory feedback"], ["Pacijent čuje samo odgođeni glas (bez jeke kad je suhi glas 0 %).", "The patient hears only the delayed voice (no echo when the dry voice is 0 %)."],
    ["Vlastiti (suhi) glas", "Own (dry) voice"], ["Odgođeni glas", "Delayed voice"], ["FAF — frekvencijski promijenjena povratna veza", "FAF — frequency-altered feedback"],
    ["Pomak visine glasa u polutonovima (±12 = oktava).", "Pitch shift in semitones (±12 = one octave)."], ["Udio pomaknutog", "Shifted share"], ["Maskirajući šum", "Masking noise"],
    ["Razina je RMS u dBFS. Uskopojasni šum: 1/3 oktave oko središnje frekvencije.", "Level is RMS in dBFS. Narrow-band noise: 1/3 octave around the centre frequency."],
    ["Vrsta", "Type"], ["Središnja frekv.", "Centre freq."], ["Uho", "Ear"], ["Diskontinuitet (prekidanje)", "Discontinuity (interruption)"],
    ["Periodično utišavanje signala — trajanje pauze na kraju svakog perioda.", "Periodic muting of the signal — the gap at the end of each period."],
    ["Glasnoća i kanali", "Volume and channels"], ["Pojačanje prije i poslije EQ-a te glasnoća po uhu (0–200 %).", "Gain before and after the EQ and volume per ear (0–200 %)."],
    ["Ulaz (mikrofon)", "Input (microphone)"], ["Ulazno pojačanje (prije EQ-a)", "Input gain (pre-EQ)"], ["Izlazno pojačanje (poslije EQ-a)", "Output gain (post-EQ)"],
    ["Lijevo uho", "Left ear"], ["Desno uho", "Right ear"], ["poveži L/D", "link L/R"], ["Lijevi → oba", "Left → both"], ["Desni → oba", "Right → both"],
    ["Uključeno — klik isključuje", "On — click to switch off"], ["Isključeno — klik uključuje", "Off — click to switch on"],
    // session sidebar
    ["Terapijska sesija", "Therapy session"], ["Klijent", "Client"], ["Status", "Status"], ["Nijedan odabran", "None selected"], ["Započni sesiju", "Start session"],
    ["Završi sesiju", "End session"], ["Bilješke sesije", "Session notes"], ["Dodaj bilješke o ovoj terapijskoj sesiji…", "Add notes about this therapy session…"],
    ["Započnite sesiju za bilješke.", "Start a session to take notes."], ["Bilješke se spremaju automatski.", "Notes are saved automatically."], ["Povijest sesija", "Session history"],
    ["Terapijske sesije", "Therapy sessions"], ["u tijeku", "in progress"], ["nema aktivne", "none active"], ["Odaberite klijenta.", "Select a client."],
    ["Odaberite klijenta za povijest sesija.", "Select a client to see the session history."], ["Nema sesija. Započnite sesiju — bilježe se postavke, snimke i analize.", "No sessions yet. Start a session — settings, recordings and analyses are recorded."],
    ["Analiza glasa", "Voice analysis"], ["Real-time", "Real-time"], ["Glasovne analize", "Voice analyses"], ["Srednja visina", "Mean pitch"], ["cijela", "whole"],
    ["Otvori analizu", "Open analysis"], ["Preuzmi", "Download"], ["Detalji sesije", "Session details"], ["Odaberite pacijenta", "Select a patient"],
    // session detail
    ["Sesija", "Session"], ["Početak", "Start"], ["Kraj", "End"], ["Trajanje", "Duration"], ["Postavke na početku sesije", "Settings at the start of the session"],
    ["Bilješke", "Notes"], ["Nema snimaka u ovoj sesiji.", "No recordings in this session."], ["Primijeni postavke ove sesije", "Apply this session's settings"],
    ["Učitava EQ, DAF, FAF, šum, diskontinuitet i glasnoću kakvi su bili na početku sesije.", "Loads the EQ, DAF, FAF, noise, discontinuity and volume as they were at the start of the session."],
    ["Za ovu sesiju nisu spremljene postavke (starija verzija).", "No settings were stored for this session (older version)."],
    ["Učitati postavke ove sesije? Trenutne postavke zvuka bit će zamijenjene.", "Load this session's settings? The current audio settings will be replaced."],
    ["uključen", "on"], ["isključen", "off"], ["Diskontinuitet", "Discontinuity"], ["Preset na početku sesije:", "Preset at the start of the session:"],
    // analysis page
    ["Snimanje i učitavanje", "Recording and upload"], ["utišaj slušalice dok je ovaj prikaz otvoren", "mute the headphones while this view is open"],
    ["Zadatak", "Task"], ["Oznaka", "Label"], ["npr. nakon 4. terapije", "e.g. after the 4th therapy"], ["Snimi", "Record"], ["Učitaj audio", "Upload audio"], ["Učitavam…", "Uploading…"],
    ["suhi mikrofon", "dry microphone"], ["obrađeni izlaz", "processed output"], ["— zadatak —", "— task —"],
    ["Za analizu glasa: produženi vokal /a/ 3–5 s, stalna udaljenost od mikrofona (npr. 30 cm), tiha prostorija. Za mucanje: čitanje i spontani govor ≥ 300 slogova. Učitajte i postojeće snimke (WAV za točnu analizu; MP3/M4A/OGG se pretvaraju).",
      "For voice analysis: a sustained vowel /a/ of 3–5 s, a constant distance to the microphone (e.g. 30 cm), a quiet room. For stuttering: reading and spontaneous speech ≥ 300 syllables. You can also upload existing recordings (WAV for exact analysis; MP3/M4A/OGG are converted)."],
    ["Produženi vokal /a/", "Sustained vowel /a/"], ["Produženi vokal /i/", "Sustained vowel /i/"], ["Produženi vokal /u/", "Sustained vowel /u/"], ["Čitanje standardnog teksta", "Reading a standard passage"],
    ["Spontani govor", "Spontaneous speech"], ["Brojanje / automatizirani govor", "Counting / automatic speech"], ["Ponavljanje rečenica", "Sentence repetition"],
    ["Glasovni raspon / glisando", "Voice range / glissando"], ["Pjevanje", "Singing"], ["Kalibracija", "Calibration"], ["Ostalo", "Other"],
    ["Snimke klijenta", "Client recordings"], ["Datum", "Date"], ["Izvor", "Source"], ["Oznake", "Labels"], ["Analize", "Analyses"], ["Otvori", "Open"], ["Obriši", "Delete"],
    ["mikrofon", "microphone"], ["obrađeno", "processed"], ["učitano", "uploaded"],
    ["Odaberite klijenta (lijevo) — snimke se spremaju uz klijenta i aktivnu sesiju.", "Select a client (left) — recordings are stored with the client and the active session."],
    ["Nema snimaka. Snimite ili učitajte audio gore.", "No recordings. Record or upload audio above."],
    ["Cijela snimka", "Whole recording"], ["Odabir", "Selection"], ["Opseg analize", "Analysis scope"], ["Analiziraj", "Analyse"], ["Spremi analizu", "Save analysis"],
    ["Spremi rezultat uz snimku i sesiju", "Save the result with the recording and the session"], ["Izvoz ▾", "Export ▾"], ["Mjere (CSV)", "Measures (CSV)"],
    ["Voice report (tekst, Praat oblik)", "Voice report (text, Praat format)"], ["Visina tona (CSV)", "Pitch listing (CSV)"], ["Formanti (CSV)", "Formant listing (CSV)"],
    ["Intenzitet (CSV)", "Intensity listing (CSV)"], ["Pulsevi (CSV)", "Pulses (CSV)"], ["Slika editora (PNG)", "Editor image (PNG)"], ["Nalaz (.txt)", "Report (.txt)"],
    ["Preuzmi WAV", "Download WAV"], ["Oznake kao Praat TextGrid", "Labels as a Praat TextGrid"], ["Ispis / PDF", "Print / PDF"], ["Ispis ili PDF nalaza", "Print or PDF of the report"],
    ["Povećaj editor", "Enlarge the editor"], ["Zatvori snimku", "Close the recording"],
    ["Povucite za odabir · kotačić = zum (Shift = pomak) · razmaknica = reprodukcija · A = oznaka · dvoklik na oznaku = uredi · Delete = obriši oznaku",
      "Drag to select · wheel = zoom (Shift = scroll) · space = play · A = label · double-click a label = edit · Delete = delete the label"],
    ["Mjere", "Measures"], ["Nalaz", "Report"], ["Anotacija", "Annotation"], ["Spremljene analize", "Saved analyses"], ["Postavke prikaza", "Display settings"], ["AI mišljenje", "AI opinion"],
    ["Mjere (Praat voice report)", "Measures (Praat voice report)"], ["Opažanja rehabilitatora za ovu snimku", "Clinician's observations on this recording"],
    ["npr. tvrdi počeci, blokovi na p/t/k, šumni glas nakon 30 s (bez imena i osobnih podataka)", "e.g. hard onsets, blocks on p/t/k, breathy voice after 30 s (no names or personal data)"],
    ["Klinički nalaz (akustička analiza)", "Clinical report (acoustic analysis)"], ["Kopiraj", "Copy"], ["Spremi .txt", "Save .txt"], ["Kopirano", "Copied"], ["Kopiranje nije dopušteno", "Copying is not allowed"],
    ["Netečnosti i oznake", "Disfluencies and labels"], ["Slogova", "Syllables"], ["Ručno prebrojani slogovi (0 = automatska procjena)", "Manually counted syllables (0 = automatic estimate)"],
    ["Spremljene analize ove snimke", "Saved analyses of this recording"], ["Nema spremljenih analiza.", "No saved analyses."],
    ["„Spremi analizu” trajno bilježi odsječak, postavke i sve mjere — vidljivo i u povijesti sesije.", "“Save analysis” permanently stores the segment, the settings and every measure — also shown in the session history."],
    ["Spremljeno", "Saved"], ["Naziv", "Name"], ["Odsječak", "Segment"], ["Visina (postavke)", "Pitch (settings)"], ["Prikaži mjere i odsječak", "Show the measures and the segment"],
    ["Obrisati spremljenu analizu?", "Delete the saved analysis?"], ["Analiza spremljena uz snimku i sesiju", "Analysis saved with the recording and the session"], ["Prvo pokrenite analizu", "Run the analysis first"],
    ["Naziv spremljene analize:", "Name of the saved analysis:"], ["cijela snimka", "whole recording"], ["Nema odabira — analiziram cijelu snimku", "No selection — analysing the whole recording"],
    ["Konture još nisu izračunate", "The contours are not computed yet"], ["Analiza još nije gotova", "The analysis is not finished yet"],
    ["Analiza…", "Analysing…"], ["Učitavanje…", "Loading…"], ["Računam Praat voice report i formante…", "Computing the Praat voice report and formants…"],
    ["Početak", "Start"], ["Kraj", "End"], ["Vrsta", "Kind"], ["Napomena", "Note"],
    ["Nema oznaka. Označite netečnosti u editoru: odaberite odsječak i pritisnite", "No labels. Mark disfluencies in the editor: select a segment and press"], ["ili „+ Oznaka“.", "or “+ Label”."],
    ["Ukupno oznaka", "Total labels"], ["Netečnosti tipične za mucanje (SLD)", "Stuttering-like disfluencies (SLD)"], ["Oznaka u minuti", "Labels per minute"],
    ["% mucanih slogova (%SS)", "% syllables stuttered (%SS)"], ["Prosjek 3 najduža SLD", "Mean of the 3 longest SLD"], ["rehabilitator", "counted by the clinician"],
    ["automatska procjena (slogovne jezgre)", "automatic estimate (syllable nuclei)"],
    ["Snimka ", "Recording "], ["Snimka", "Recording"], ["Spremljeno:", "Saved:"], ["Otvaram analizu", "Opening the analysis"],
    ["Nije odabran klijent — snimiti bez klijenta?", "No client selected — record without a client?"], ["Nije odabran klijent — učitati snimku bez klijenta?", "No client selected — upload the recording without a client?"],
    // tiles + measures
    ["F0 prosjek", "F0 mean"], ["Jitter (local)", "Jitter (local)"], ["Shimmer (local)", "Shimmer (local)"], ["HNR", "HNR"], ["CPPS", "CPPS"], ["Intenzitet", "Intensity"],
    ["Najduža fonacija", "Longest phonation"], ["MPT", "MPT"], ["viši = periodičniji glas", "higher = more periodic voice"], ["računa se…", "computing…"],
    ["kalibrirano", "calibrated"], ["relativno (nekalibrirano)", "relative (uncalibrated)"],
    ["Mjera", "Measure"], ["Vrijednost", "Value"], ["Orijent. granica", "Indicative limit"],
    ["Visina (Pitch)", "Pitch"], ["Medijan F0", "F0 median"], ["Prosjek F0", "F0 mean"], ["Standardna devijacija F0", "F0 standard deviation"], ["Minimum / maksimum F0", "F0 minimum / maximum"],
    ["Raspon F0", "F0 range"], ["Pulsevi (Pulses)", "Pulses"], ["Broj pulseva", "Number of pulses"], ["Broj perioda", "Number of periods"], ["Prosječni period", "Mean period"], ["SD perioda", "SD of period"],
    ["Zvučnost (Voicing)", "Voicing"], ["Udio nezvučnih okvira", "Fraction of unvoiced frames"], ["Broj prekida zvučnosti", "Number of voice breaks"], ["Stupanj prekida zvučnosti", "Degree of voice breaks"],
    ["Harmoničnost (Harmonicity)", "Harmonicity"], ["Srednja autokorelacija", "Mean autocorrelation"], ["Omjer šum/harmonici (NHR)", "Noise-to-harmonics ratio (NHR)"], ["Omjer harmonici/šum (HNR)", "Harmonics-to-noise ratio (HNR)"],
    ["Kepstralna analiza", "Cepstral analysis"], ["CPPS (AVQI postavke)", "CPPS (AVQI settings)"], ["Formanti (medijan zvučnih okvira)", "Formants (median of voiced frames)"],
    ["Prosjek", "Mean"], ["Minimum / maksimum", "Minimum / maximum"], ["Standardna devijacija", "Standard deviation"], ["Razina prema digitalnom maksimumu", "Level relative to digital full scale"],
    ["Vremenske mjere", "Timing measures"], ["Najduža fonacija (MPT)", "Longest phonation (MPT)"], ["Pauze ≥ 250 ms", "Pauses ≥ 250 ms"], ["Prosječna pauza", "Mean pause"], ["Udio pauza", "Pause ratio"],
    ["Slogovne jezgre", "Syllable nuclei"], ["Brzina govora", "Speech rate"], ["Brzina artikulacije", "Articulation rate"], ["slog/s", "syll/s"], ["dB (rel.)", "dB (rel.)"],
    ["Algoritmi su port Praata (Boersma & Weenink), provjereni prema Praat 7.0. Granice su orijentacijske (MDVP/literatura) i ovise o zadatku, mikrofonu i prostoriji.",
      "The algorithms are a port of Praat (Boersma & Weenink), verified against Praat 7.0. Limits are indicative (MDVP/literature) and depend on the task, microphone and room."],
    // AI
    ["AI postavke", "AI settings"], ["⚙ AI postavke", "⚙ AI settings"], ["Pokreni AI analizu", "Run AI analysis"], ["Pregled podataka za AI", "Preview the data for the AI"],
    ["priloži sliku editora (sonagram + konture)", "attach the editor image (spectrogram + contours)"],
    ["Dodatno pitanje ili fokus (neobavezno), npr. „Usporedi s prethodnom snimkom“ ili „Je li DAF prikladan?“", "Extra question or focus (optional), e.g. “Compare with the previous recording” or “Is DAF suitable?”"],
    ["AI ne čuje snimku: dobiva izmjerene parametre, profil pacijenta bez imena, šifre i datuma rođenja, vaša opažanja i (po izboru) sliku sonagrama. Mišljenje je pomoć rehabilitatoru, ne dijagnoza.",
      "The AI does not hear the recording: it gets the measured parameters, the patient profile without name, code or birth date, your observations and (optionally) the spectrogram image. The opinion supports the clinician; it is not a diagnosis."],
    ["AI postavke nisu učitane.", "AI settings are not loaded."], ["AI model nije postavljen — otvorite ⚙ AI postavke.", "No AI model set — open ⚙ AI settings."],
    ["Nedostaje Anthropic API ključ — otvorite ⚙ AI postavke.", "The Anthropic API key is missing — open ⚙ AI settings."], ["Pacijent nema zabilježenu suglasnost za AI analizu (Uredi pacijenta).", "The patient has no recorded consent for AI analysis (Edit patient)."],
    ["Ollama nije dostupna.", "Ollama is not reachable."], ["Ollama (lokalno)", "Ollama (local)"], ["OpenAI-kompatibilan", "OpenAI-compatible"], ["Anthropic Claude", "Anthropic Claude"],
    ["AI radi…", "AI working…"], ["Pripremam…", "Preparing…"], ["Log", "Log"], ["■ Zaustavi", "■ Stop"], ["AI piše…", "The AI is writing…"], ["Djelomičan odgovor (nije spremljen)", "Partial answer (not saved)"],
    ["Djelomičan odgovor:", "Partial answer:"], ["✓ Gotovo", "✓ Done"], ["✗ Greška", "✗ Error"], ["■ Zaustavljeno", "■ Stopped"], ["Ranija AI mišljenja:", "Earlier AI opinions:"],
    ["Kopiraj AI mišljenje", "Copy the AI opinion"], ["Obrisati ovo AI mišljenje?", "Delete this AI opinion?"], ["Odgovor je skraćen (dosegnuto ograničenje duljine).", "The answer was cut (length limit reached)."],
    ["AI servis", "AI service"], ["Adresa", "Address"], ["API ključ", "API key"], ["šalji sliku sonagrama", "send the spectrogram image"], ["Test veze", "Test connection"], ["Obriši ključ", "Delete key"],
    ["Testiram…", "Testing…"], ["Testiram… (učitavanje modela može potrajati)", "Testing… (loading the model can take a while)"], ["AI postavke spremljene", "AI settings saved"], ["Ključ obrisan", "Key deleted"],
    ["Obrisati spremljeni API ključ?", "Delete the stored API key?"], ["Promjena servisa briše spremljeni API ključ prethodnog servisa. Nastaviti?", "Changing the service deletes the stored API key of the previous one. Continue?"],
    ["OpenAI-kompatibilan (OpenAI, LM Studio, vLLM…)", "OpenAI-compatible (OpenAI, LM Studio, vLLM…)"], ["Ollama (lokalno, besplatno, podaci ne napuštaju računalo)", "Ollama (local, free, data stays on the computer)"],
    ["•••• spremljen (upišite za promjenu)", "•••• stored (type to change)"], ["sk-… (prazno za lokalne servere)", "sk-… (empty for local servers)"],
    ["Odaberite instalirani model ili preuzmite preporučeni. Za sliku sonagrama treba model s vidom (👁).", "Pick an installed model or download a recommended one. The spectrogram image needs a vision model (👁)."],
    ["Bilo koji servis s OpenAI /chat/completions sučeljem (OpenAI, Azure proxy, LM Studio, vLLM, OpenRouter…). Upišite model i adresu.", "Any service with an OpenAI /chat/completions interface (OpenAI, Azure proxy, LM Studio, vLLM, OpenRouter…). Enter the model and the address."],
    ["Kontekst", "Context"], ["automatski (prema upitu)", "automatic (from the prompt)"], ["⟳ Osvježi", "⟳ Refresh"], ["Osvježi", "Refresh"], ["Instalirani modeli", "Installed models"], ["Preporučeni modeli", "Recommended models"],
    ["ili naziv s ollama.com/library, npr. qwen2.5:7b", "or a name from ollama.com/library, e.g. qwen2.5:7b"], ["⬇ Preuzmi", "⬇ Download"], ["✓ preuzet", "✓ downloaded"], ["Koristi", "Use"], ["✓ u upotrebi", "✓ in use"],
    ["Obriši model s diska", "Delete the model from disk"], ["Provjeravam Ollamu…", "Checking Ollama…"], ["Nema preuzetih modela — preuzmite jedan desno.", "No downloaded models — download one on the right."],
    ["✓ Preuzeto", "✓ Downloaded"], ["Preuzimanje prekinuto", "Download stopped"], ["👁 slika", "👁 image"], ["Čita slike (vision)", "Reads images (vision)"],
    ["Ollama radi na ovom računalu (ili drugom u mreži) — besplatno, bez API ključa, podaci ne izlaze van. Brzina ovisi o grafičkoj kartici: model mora stati u njezinu memoriju (VRAM), inače radi na procesoru i znatno je sporiji.",
      "Ollama runs on this computer (or another one on the network) — free, no API key, data does not leave. Speed depends on the graphics card: the model must fit into its memory (VRAM), otherwise it runs on the CPU and is much slower."],
    ["preporuka za GPU sa 6 GB (npr. GTX 1060 6 GB) — dobar hrvatski, čita sliku", "recommended for 6 GB GPUs (e.g. GTX 1060 6 GB) — good Croatian, reads images"],
    ["mali model s vidom, GPU 4–6 GB", "small vision model, 4–6 GB GPU"], ["najbrži, bez slike; GPU 3–4 GB ili samo CPU", "fastest, no images; 3–4 GB GPU or CPU only"],
    ["jači tekst, bez slike; GPU 6–8 GB", "stronger text, no images; 6–8 GB GPU"], ["jači model s vidom; GPU 8 GB+", "stronger vision model; 8 GB+ GPU"], ["kvalitetniji; GPU 12 GB+", "higher quality; 12 GB+ GPU"],
    ["11B s vidom; GPU 12 GB+, na slabijem hardveru vrlo spor", "11B with vision; 12 GB+ GPU, very slow on weaker hardware"], ["AI ⏳", "AI ⏳"],
    // settings dialog
    ["Postavke", "Settings"], ["Ustanova (zaglavlje nalaza)", "Clinic (report header)"], ["Naziv ustanove", "Clinic name"], ["Rehabilitator / logoped", "Clinician / speech therapist"],
    ["npr. Poliklinika …", "e.g. Polyclinic …"], ["ime, titula", "name, title"], ["Zadane postavke analize (Praat)", "Default analysis settings (Praat)"], ["Odrasli muški", "Adult male"], ["Odrasli ženski", "Adult female"], ["Dijete", "Child"],
    ["Standard (Praat)", "Standard (Praat)"], ["Visina — donja granica (Hz)", "Pitch — floor (Hz)"], ["Visina — gornja granica (Hz)", "Pitch — ceiling (Hz)"], ["Maksimalni formant (Hz)", "Maximum formant (Hz)"],
    ["Broj formanata", "Number of formants"], ["računaj CPPS (sporije na dugim snimkama)", "compute CPPS (slower on long recordings)"], ["Kalibracija mikrofona (dB SPL)", "Microphone calibration (dB SPL)"],
    ["Bez kalibracije intenzitet je relativan (Praat dB, 1 = 1 Pa). Snimite kalibrator ili zvuk poznate razine na istom mjestu mikrofona, zatim unesite razinu i odaberite snimku.",
      "Without calibration the intensity is relative (Praat dB, 1 = 1 Pa). Record a calibrator or a sound of known level at the same microphone position, then enter the level and choose the recording."],
    ["mikrofon je kalibriran", "the microphone is calibrated"], ["Pomak (dB SPL = Praat dB + pomak)", "Offset (dB SPL = Praat dB + offset)"], ["Poznata razina (dB SPL)", "Known level (dB SPL)"], ["npr. 94", "e.g. 94"],
    ["Snimka kalibratora", "Calibrator recording"], ["Izračunaj pomak iz snimke", "Compute the offset from the recording"], ["Postavke vrijede za sve korisnike ovog računala.", "The settings apply to all users of this computer."],
    ["— odaberite snimku —", "— choose a recording —"], ["— nema snimaka —", "— no recordings —"], ["bez pacijenta", "no patient"], ["Mjerim…", "Measuring…"], ["Postavke spremljene", "Settings saved"],
    ["Odaberite snimku i unesite poznatu razinu u dB SPL", "Choose a recording and enter the known level in dB SPL"], ["snimka je pretiha za mjerenje", "the recording is too quiet to measure"],
    // keys dialog
    ["Razmaknica", "Space"], ["pokreni / zaustavi zvuk (u editoru: reproduciraj odabir)", "start / stop audio (in the editor: play the selection)"], ["reset EQ na 0 dB", "reset the EQ to 0 dB"],
    ["osvježi popis audio uređaja", "refresh the audio device list"], ["utišaj / uključi izlaz", "mute / unmute the output"], ["tabovi Rehabilitacija / Analiza glasa / Napredak", "pages Real-time / Voice analysis / Progress"],
    ["traži pacijenta", "search patients"], ["zatvori prozor", "close the window"], ["Editor analize:", "Analysis editor:"], ["zum,", "zoom,"], ["pomak,", "scroll,"], ["nova oznaka,", "new label,"], ["briše odabranu oznaku.", "deletes the selected label."],
    // patient dialog
    ["Pacijent", "Patient"], ["Novi pacijent", "New patient"], ["Pacijent — uredi", "Patient — edit"], ["Ime i prezime *", "Full name *"], ["Šifra / MBO", "Code / ID"], ["Datum rođenja", "Date of birth"],
    ["Spol", "Sex"], ["Ž", "F"], ["M", "M"], ["drugo", "other"], ["Materinski jezik", "Native language"], ["npr. hrvatski", "e.g. Croatian"], ["Pušenje", "Smoking"], ["nepušač", "non-smoker"], ["bivši pušač", "former smoker"], ["pušač", "smoker"],
    ["Dijagnoza / MKB-10", "Diagnosis / ICD-10"], ["npr. R49.0 disfonija; F98.5 mucanje", "e.g. R49.0 dysphonia; F98.5 stuttering"], ["Trenutni problem / razlog dolaska", "Current complaint / reason for referral"],
    ["Anamneza problema (početak, trajanje, tijek, dosadašnja terapija)", "History of the problem (onset, duration, course, previous therapy)"], ["Medicinska povijest (operacije, neurološko, refluks, hormonalno…)", "Medical history (surgery, neurological, reflux, hormonal…)"],
    ["Lijekovi", "Medications"], ["Zanimanje i vokalno opterećenje", "Occupation and vocal load"], ["Sluh / slušni status", "Hearing / hearing status"], ["Ciljevi terapije", "Therapy goals"],
    ["Pacijent je dao suglasnost za obradu pseudonimiziranih podataka putem AI servisa", "The patient consented to processing of pseudonymised data by an AI service"], ["Obriši pacijenta", "Delete patient"],
    ["✓ suglasnost za AI", "✓ AI consent"], ["✗ nema suglasnosti za AI", "✗ no AI consent"], ["Bez sesija", "No sessions"], ["Nema rezultata.", "No results."], ["Nema pacijenata — dodajte ih gumbom + Novi.", "No patients — add one with + New."],
    ["Sesija u tijeku", "Session in progress"],
    // progress
    ["Napredak kroz vrijeme", "Progress over time"], ["Tablica mjerenja (cijele snimke, klik otvara analizu)", "Measurement table (whole recordings, click opens the analysis)"], ["Računam mjere za sve snimke (prvi put može potrajati)…", "Computing measures for all recordings (the first time can take a while)…"],
    ["Nema snimaka za prikaz.", "No recordings to show."], ["jedno mjerenje", "one measurement"], ["Traj. s", "Dur. s"], ["Slog/s", "Syll/s"], ["Ozn.", "Lab."], ["Int. dB", "Int. dB"], ["MPT s", "MPT s"],
    ["F0 Hz", "F0 Hz"], ["% mucanih slogova (%SS)", "% syllables stuttered (%SS)"], ["SLD (broj)", "SLD (count)"],
    // editor
    ["▶ Odabir", "▶ Selection"], ["▶ Vidljivo", "▶ Visible"], ["⤢ Odabir", "⤢ Selection"], ["Sve", "All"], ["Spektrogram", "Spectrogram"], ["Visina", "Pitch"], ["Formanti", "Formants"], ["Pulsevi", "Pulses"],
    ["Presjek", "Slice"], ["＋ Oznaka", "＋ Label"], ["Analiziraj odabir", "Analyse selection"], ["Reproduciraj odabir (razmaknica)", "Play the selection (space)"], ["Reproduciraj vidljivo", "Play the visible part"],
    ["Zaustavi", "Stop"], ["Zumiraj na odabir", "Zoom to the selection"], ["Povećaj (+)", "Zoom in (+)"], ["Smanji (−)", "Zoom out (−)"], ["Cijela snimka", "Whole recording"], ["Pomakni lijevo (←)", "Scroll left (←)"], ["Pomakni desno (→)", "Scroll right (→)"],
    ["Označi odabir (A)", "Label the selection (A)"], ["Analiza odabira (Praat voice report)", "Analyse the selection (Praat voice report)"], ["Postavke prikaza i analize", "Display and analysis settings"],
    ["Spektralni presjek", "Spectral slice"], ["LTAS odabira", "LTAS of the selection"], ["Računam konture (Praat)…", "Computing contours (Praat)…"], ["frekvencija (Hz)", "frequency (Hz)"], ["oznake", "labels"],
    ["Nova oznaka", "New label"], ["Uredi oznaku", "Edit label"], ["npr. glas /p/, 3 ponavljanja", "e.g. sound /p/, 3 repetitions"], ["Prvo označite odsječak mišem (povucite u prikazu)", "First select a segment with the mouse (drag in the view)"],
    ["Blok", "Block"], ["Produljenje glasa", "Sound prolongation"], ["Ponavljanje glasa", "Sound repetition"], ["Ponavljanje sloga", "Syllable repetition"], ["Ponavljanje jednosložne riječi", "Monosyllabic word repetition"],
    ["Umetak / poštapalica", "Interjection / filler"], ["Revizija / prekinuta riječ", "Revision / broken word"], ["Tvrdi početak fonacije", "Hard glottal onset"], ["Prekid / pucanje glasa", "Voice break / pitch break"],
    ["Šapat / afonija", "Whisper / aphonia"], ["Prateće ponašanje", "Secondary behaviour"],
    ["Spektrogram", "Spectrogram"], ["Raspon prikaza do (Hz)", "View range up to (Hz)"], ["Duljina prozora (s)", "Window length (s)"], ["0,003", "0.003"], ["0,005 — širokopojasni", "0.005 — wideband"], ["0,010", "0.010"], ["0,015", "0.015"],
    ["0,030 — uskopojasni", "0.030 — narrowband"], ["0,050", "0.050"], ["Oblik prozora", "Window shape"], ["Gaussov (Praat)", "Gaussian (Praat)"], ["Hann", "Hann"], ["Autoskaliranje", "Autoscaling"], ["Maksimum (dB)", "Maximum (dB)"],
    ["Pre-emphasis (dB/okt)", "Pre-emphasis (dB/oct)"], ["Dinamička kompresija (0–1)", "Dynamic compression (0–1)"], ["Dinamički raspon (dB)", "Dynamic range (dB)"], ["Visina tona (Pitch)", "Pitch"], ["Muški 60–300", "Male 60–300"], ["Ženski 100–500", "Female 100–500"],
    ["Dječji 150–700", "Child 150–700"], ["Standard 75–600", "Standard 75–600"], ["Donja granica (Hz)", "Floor (Hz)"], ["Gornja granica (Hz)", "Ceiling (Hz)"], ["Jedinica prikaza", "Display unit"], ["polutonovi (re 100 Hz)", "semitones (re 100 Hz)"],
    ["Crtanje", "Drawing"], ["linija", "line"], ["točke (speckles)", "speckles"], ["Granice mijenjaju i mjerenje (Praat „pitch floor/ceiling“).", "The limits also change the measurement (Praat “pitch floor/ceiling”)."], ["Formanti (Burg)", "Formants (Burg)"],
    ["Muški 5000", "Male 5000"], ["Ženski 5500", "Female 5500"], ["Dječji 8000", "Child 8000"], ["Prikaži formanata", "Formants shown"], ["Samo širina pojasa do (Hz, 0 = sve)", "Only bandwidth up to (Hz, 0 = all)"], ["Veličina točke", "Dot size"],
    ["Intenzitet (prikaz)", "Intensity (display)"], ["Od (dB)", "From (dB)"], ["Do (dB)", "To (dB)"], ["CPPS u analizi", "CPPS in the analysis"], ["Primijeni", "Apply"], ["Zadano", "Default"],
    ["Postavke prikaza pamte se u ovom pregledniku; granice visine i formanata vrijede za ovu snimku (zadane: ⚙ Postavke).", "Display settings are kept in this browser; pitch and formant limits apply to this recording (defaults: ⚙ Settings)."],
    // print report
    ["Akustička analiza glasa i govora", "Acoustic analysis of voice and speech"], ["Ispis / spremi kao PDF", "Print / save as PDF"], ["Ime i prezime", "Full name"], ["Razlog dolaska", "Reason for referral"], ["Dijagnoza", "Diagnosis"],
    ["Datum snimanja", "Recording date"], ["Analizirani odsječak", "Analysed segment"], ["Frekvencija uzorkovanja", "Sample rate"], ["Postavke analize", "Analysis settings"], ["Opažanja rehabilitatora", "Clinician's observations"],
    ["Oscilogram, spektrogram i konture", "Waveform, spectrogram and contours"], ["Netečnosti", "Disfluencies"], ["Po vrstama", "By kind"], ["Klinički nalaz (automatski)", "Clinical report (automatic)"],
    ["AI mišljenje — pomoć rehabilitatoru, nije dijagnoza", "AI opinion — support for the clinician, not a diagnosis"], ["nije kalibrirano (intenzitet relativan)", "not calibrated (relative intensity)"],
    ["Preglednik je blokirao novi prozor — dopustite skočne prozore", "The browser blocked the new window — allow pop-ups"],
    // server messages (errors)
    ["ime je obavezno", "the name is required"], ["predugačak unos", "entry too long"], ["no such patient", "no such patient"], ["already recording", "already recording"],
    ["audio is not running", "audio is not running"], ["not recording", "not recording"], ["sesija nema spremljene postavke", "the session has no stored settings"], ["neispravan naziv modela", "invalid model name"],
    ["nema tog zadatka", "no such task"], ["nema te analize", "no such analysis"], ["previše oznaka", "too many labels"], ["neispravna oznaka", "invalid label"], ["nepoznat AI servis", "unknown AI service"],
    ["adresa mora počinjati s http:// ili https://", "the address must start with http:// or https://"], ["naziv preseta je obavezan (do 120 znakova)", "a preset name is required (up to 120 characters)"],
    ["snimka je u međuvremenu obrisana", "the recording has been deleted meanwhile"], ["previše spremljenih analiza za ovu snimku", "too many saved analyses for this recording"],
    ["pacijent nema zabilježenu suglasnost za AI analizu (Uredi pacijenta)", "the patient has no recorded consent for AI analysis (Edit patient)"],
  ];
  // [Croatian pattern, English replacement] for texts with numbers or names inside.
  const PATTERNS = [
    [/^(\d+) god\.$/, "$1 y"], [/^Zadnja sesija: (.+)$/, "Last session: $1"], [/^(\d+) snim\.$/, "$1 rec."],
    [/^Preset: (.+) • izmijenjeno$/, "Preset: $1 • modified"], [/^Preset: (.+)$/, "Preset: $1"], [/^(.+) \(izmijenjen\)$/, "$1 (modified)"],
    [/^Aktivna sesija: (.+)$/, "Active session: $1"], [/^aktivna: (.+)$/, "active: $1"], [/^Preset na početku sesije: (.+)$/, "Preset at the start of the session: $1"],
    [/^Sesija (.+) — (.+)$/, "Session $1 — $2"], [/^Sesija (\d+)$/, "Session $1"], [/^Snimke \((\d+)\)$/, "Recordings ($1)"],
    [/^Zaustavi {2}(.+)$/, "Stop  $1"], [/^Spremljeno: (.+) s — otvaram analizu$/, "Saved: $1 s — opening the analysis"], [/^Učitano: (.+) s \(pretvoreno u WAV\)$/, "Uploaded: $1 s (converted to WAV)"],
    [/^Učitano: (.+) s$/, "Uploaded: $1 s"], [/^Učitano: (.+)$/, "Loaded: $1"], [/^Učitavanje: (.+)$/, "Upload: $1"], [/^Greška: (.+)$/, "Error: $1"], [/^Konture: (.+)$/, "Contours: $1"],
    [/^Model: (.+)$/, "Model: $1"], [/^Zadnji pokušaj: (.+)$/, "Last attempt: $1"],
    [/^odsječak (.+) s · visina (.+) Hz · CPPS se računa…$/, "segment $1 s · pitch $2 Hz · computing CPPS…"], [/^odsječak (.+) s · visina (.+) Hz$/, "segment $1 s · pitch $2 Hz"],
    [/^spremljeno (.+) · (.+) s$/, "saved $1 · $2 s"], [/^granica < (.+)$/, "limit < $1"], [/^granica > (.+)$/, "limit > $1"], [/^SD (.+) Hz$/, "SD $1 Hz"],
    [/^(\d+) snimaka od (.+) do (.+) Uspoređujte isti zadatak \(npr\. samo produženi vokal \/a\/\) — odaberite ga gore\.$/, (m, a, b, c) => `${a} recordings from ${b} to ${c}`.replace(/\.$/, "") + ". Compare the same task (e.g. only the sustained vowel /a/) — choose it above."],
    [/^Sve snimke \((\d+)\)$/, "All recordings ($1)"], [/^od prve: (.+?)( · granica (.) (.+))?$/, (m, a, b, c, d) => `since the first: ${a}${b ? ` · limit ${c} ${d}` : ""}`],
    [/^jedno mjerenje · granica (.) (.+)$/, "one measurement · limit $1 $2"],
    [/^kursor (.+) s · odabir (.+)–(.+) s \((.+) s\) · prikaz (.+)–(.+) s$/, "cursor $1 s · selection $2–$3 s ($4 s) · view $5–$6 s"], [/^Spektralni presjek @ (.+) s$/, "Spectral slice @ $1 s"],
    [/^LTAS (\d+) okvira · (.+) s$/, "LTAS $1 frames · $2 s"], [/^int\. (.+) dB$/, "int. $1 dB"],
    [/^F0 (.+) · intenzitet (.+)$/, "F0 $1 · intensity $2"], [/^−(.+)s$/, "−$1s"],
    [/^● Ollama (.+) radi na (.+) · (\d+) modela?$/, "● Ollama $1 running at $2 · $3 models"], [/^U memoriji: (.+)$/, (m, a) => "In memory: " + a.replace(/samo CPU/g, "CPU only")],
    [/^Preuzimam (.+)$/, "Downloading $1"], [/^✗ Greška: (.+)$/, "✗ Error: $1"], [/^kontekst (\d+)$/, "context $1"],
    [/^≈ (.+) GB$/, "≈ $1 GB"], [/^Obrisati model (.+) iz Ollame \((.+) GB\)\?$/, "Delete model $1 from Ollama ($2 GB)?"],
    [/^Model „(.+)” nije preuzet — ⚙ AI postavke → Preuzmi\.$/, "Model “$1” is not downloaded — ⚙ AI settings → Download."],
    [/^Model (.+) nema vid \(vision\): slika sonagrama se neće poslati, samo izmjereni podaci\.$/, "Model $1 has no vision: the spectrogram image will not be sent, only the measured data."],
    [/^Pseudonimizirani podaci šalju se vanjskom servisu \((.+)\)\. Za potpuno lokalnu obradu odaberite Ollama\.$/, "Pseudonymised data is sent to an external service ($1). For fully local processing choose Ollama."],
    [/^⏳ (.+) · model učitava i čita upit…$/, "⏳ $1 · the model is loading and reading the prompt…"], [/^✍ (.+) · (\d+) tokena(.*)$/, "✍ $1 · $2 tokens$3"],
    [/^(✓ Gotovo|✗ Greška|■ Zaustavljeno) nakon (.+)$/, (m, a, b) => ({ "✓ Gotovo": "✓ Done", "✗ Greška": "✗ Error", "■ Zaustavljeno": "■ Stopped" }[a] + " after " + b.replace(/ tokena/, " tokens"))],
    [/^Servis: (.+) \((.+)\) · model: (.+) · slika: (da|ne)$/, (m, a, b, c, d) => `Service: ${a} (${b}) · model: ${c} · image: ${d === "da" ? "yes" : "no"}`],
    [/^✓ Veza radi — model (.+) je odgovorio: „(.+)“$/, "✓ Connection works — model $1 answered: “$2”"],
    [/^Trajno obrisati pacijenta "(.+)" sa svim sesijama, snimkama i AI mišljenjima\?$/, 'Permanently delete patient "$1" with all sessions, recordings and AI opinions?'],
    [/^Obrisati snimku "(.+)" \((.+)\) s oznakama i AI mišljenjima\?$/, 'Delete recording "$1" ($2) with its labels and AI opinions?'], [/^Obrisati preset "(.+)"\?$/, 'Delete preset "$1"?'],
    [/^Izmjereno (.+) dB \(Praat\) → pomak (.+) dB\. Spremite postavke\.$/, "Measured $1 dB (Praat) → offset $2 dB. Save the settings."],
    [/^neispravno vrijeme oznake (.+)$/, "invalid label time $1"], [/^nepoznata vrsta oznake: (.+)$/, "unknown label kind: $1"], [/^datoteka nije ispravan WAV: (.+)$/, "the file is not a valid WAV: $1"],
    [/^snimka je prazna ili ima prenisku frekvenciju uzorkovanja \((.+)\)$/, "the recording is empty or its sample rate is too low ($1)"],
    [/^(.+) god\. \((\d+) god\.\)$/, "$1 ($2 y)"], [/^nalaz_(.*)$/, "report_$1"], [/^Ispisano (.+)$/, "Printed $1"],
    [/^dB SPL = Praat dB ([+−]) (.+) dB$/, "dB SPL = Praat dB $1 $2 dB"], [/^visina (.+) Hz · maks\. formant (.+) Hz · (.+) formanata$/, "pitch $1 Hz · maximum formant $2 Hz · $3 formants"],
    [/^(.+) s \((.+) s od (.+) s\)$/, "$1 s ($2 s of $3 s)"], [/^(\d+) \((.+)\)$/, "$1 ($2)"],
  ];
  // Music texts are written in English: in Croatian they are translated back.
  const EN_PATTERNS = [
    [/^(\d+) bands$/, "$1 pojaseva"], [/^Replace the current board with "(.+)"\?\nSave it as a preset first if you want to keep it\.$/, 'Zamijeniti trenutni board s "$1"?\nPrije toga ga spremite kao preset ako ga želite zadržati.'],
    [/^Delete preset "(.+)"\?$/, 'Obrisati preset "$1"?'], [/^default \((.+)\)$/, "zadano ($1)"],
  ];
  PAIRS.push(
    ["▶ Pokreni zvuk", "▶ Start audio"], ["Nema aktivne sesije", "No active session"], ["Greška:", "Error:"], ["analiza nije uspjela", "the analysis failed"],
    ["Ollama nije dostupna", "Ollama is not available"], ["Ollama nije dostupna.", "Ollama is not available."], ["✗ Ollama nije dostupna", "✗ Ollama is not available"],
    ["Odaberite pacijenta.", "Select a patient."], ["Slušna povratna veza, EQ, DAF/FAF, šum", "Auditory feedback, EQ, DAF/FAF, noise"],
    ["Slušna povratna veza: EQ, DAF, FAF, šum, diskontinuitet", "Auditory feedback: EQ, DAF, FAF, noise, interruption"],
    ["Zadani model:", "Default model:"], [". Ključ: console.anthropic.com → API Keys. Ključ se sprema samo na ovom računalu (ai.json) i nikad se ne prikazuje u pregledniku.", ". Key: console.anthropic.com → API Keys. The key is stored only on this computer (ai.json) and is never shown in the browser."],
    ["Algoritmi su port Praata (Boersma & Weenink), provjereni prema Praat 7.0. Granice su orijentacijske (MDVP/literatura) i ovise o zadatku, mikrofonu i prostoriji.", "The algorithms are a port of Praat (Boersma & Weenink), verified against Praat 7.0. The limits are indicative (MDVP/literature) and depend on the task, microphone and room."],
    ["Ranija AI mišljenja:", "Earlier AI opinions:"], ["Kopirano", "Copied"], ["Kopiranje nije dopušteno", "Copying is not allowed"], ["✓ u upotrebi", "✓ in use"], ["Koristi", "Use"], ["✓ preuzet", "✓ downloaded"], ["Test veze", "Test connection"],
    ["Ispis / spremi kao PDF", "Print / save as PDF"], ["Akustička analiza glasa i govora", "Acoustic voice and speech analysis"], ["Ime i prezime", "Full name"], ["Šifra / MBO", "Code / insurance no."],
    ["Datum rođenja", "Date of birth"], ["Spol", "Sex"], ["Dijagnoza", "Diagnosis"], ["Razlog dolaska", "Reason for referral"], ["Pacijent", "Patient"], ["Snimka", "Recording"], ["Zadatak", "Task"], ["Oznaka", "Label"],
    ["Datum snimanja", "Recording date"], ["Izvor", "Source"], ["obrađeni izlaz", "processed output"], ["suhi mikrofon", "dry microphone"], ["Analizirani odsječak", "Analysed selection"], ["Frekvencija uzorkovanja", "Sample rate"],
    ["Postavke analize", "Analysis settings"], ["Kalibracija", "Calibration"], ["nije kalibrirano (intenzitet relativan)", "not calibrated (relative intensity)"], ["Opažanja rehabilitatora", "Clinician's observations"],
    ["Oscilogram, spektrogram i konture", "Oscillogram, spectrogram and contours"], ["Mjere", "Measures"], ["Netečnosti", "Disfluencies"], ["Ukupno oznaka", "Total labels"], ["Slogova", "Syllables"],
    ["Prosjek 3 najduža SLD", "Mean of the 3 longest SLD"], ["Po vrstama", "By kind"], ["Klinički nalaz (automatski)", "Clinical report (automatic)"], ["AI mišljenje — pomoć rehabilitatoru, nije dijagnoza", "AI opinion — an aid for the clinician, not a diagnosis"],
    ["pacijent", "patient"], ["previše spremljenih analiza za ovu snimku", "too many saved analyses for this recording"], ["nema te analize", "no such analysis"],
    ["naziv preseta je obavezan (do 120 znakova)", "a preset name is required (up to 120 characters)"], ["tvornički preseti se ne mogu brisati", "factory presets cannot be deleted"],
    ["pacijent nema zabilježenu suglasnost za AI analizu (Uredi pacijenta)", "the patient has no recorded consent for AI analysis (Edit patient)"], ["snimka je u međuvremenu obrisana", "the recording has been deleted in the meantime"],
    ["nema tog zadatka", "no such task"], ["ime je obavezno", "a name is required"], ["predugačak unos", "input too long"], ["previše oznaka", "too many labels"], ["neispravna oznaka", "invalid label"], ["snimka", "recording"], ["Datum", "Date"], ["Rehabilitator / logoped", "Clinician / speech therapist"], ["sesija", "session"], ["Nema snimaka za prikaz.", "No recordings to show."],
  );
  PATTERNS.push(
    [/^Greška: ([\s\S]+)$/, "Error: $1"], [/^Snimke \((\d+)\)$/, "Recordings ($1)"], [/^Sve snimke \((\d+)\)$/, "All recordings ($1)"],
    [/^(\d+) snimaka od (.+) do (.+) Uspoređujte isti zadatak \(npr\. samo produženi vokal \/a\/\) — odaberite ga gore\.$/, (m, a, b, c) => `${a} recordings from ${b} to ${c}`.replace(/\.$/, "") + ". Compare the same task (e.g. only the sustained vowel /a/) — choose it above."],
    [/^Učitano: (.+?) s \(pretvoreno u WAV\)$/, "Loaded: $1 s (converted to WAV)"], [/^Učitano: (.+)$/, "Loaded: $1"], [/^Učitavanje: (.+)$/, "Upload: $1"], [/^Zadnji pokušaj: (.+)$/, "Last attempt: $1"],
    [/^aktivna: (.+)$/, "active: $1"], [/^(\d+) sesija$/, (m, n) => n + (n === "1" ? " session" : " sessions")],
    [/^odsječak (.+) s · visina (.+) Hz( · CPPS se računa…)?$/, (m, a, b, c) => `selection ${a} s · pitch ${b} Hz` + (c ? " · computing CPPS…" : "")],
    [/^Algoritmi su port Praata .* prostoriji\.( \* .*)?$/, (m, a) => "The algorithms are a port of Praat (Boersma & Weenink), verified against Praat 7.0. The limits are indicative (MDVP/literature) and depend on the task, microphone and room." + (a || "")],
    [/^SterOidSoundBoard (.*) · DigiLingua\. Mjere su izračunate Praat-kompatibilnim algoritmima\. Nalaz je pomoćno sredstvo i ne zamjenjuje kliničku procjenu\.$/, "SterOidSoundBoard $1 · DigiLingua. The measures are computed with Praat-compatible algorithms. The report is an aid and does not replace clinical assessment."],
    [/^Nalaz — (.+)$/, "Report — $1"], [/^([\d.,—-]+) slog\/s$/, "$1 syll/s"], [/^zadano \((.+)\)$/, (m, a) => `default (${a === "nema" ? "none" : a})`], [/^(.+) \((\d+) god\.\)$/, "$1 ($2 y)"],
  );
  const H2E = new Map(), E2H = new Map();
  for (const [h, e] of PAIRS) { if (!H2E.has(h)) H2E.set(h, e); if (!E2H.has(e)) E2H.set(e, h); }
  // the Music panel's own English words
  [["Music", "Glazba"], ["Audio", "Zvuk"], ["Presets", "Preseti"], ["Mute", "Utišaj"], ["Muted", "Utišano"], ["Driver", "Upravljački program"], ["Input", "Ulaz"], ["Output", "Izlaz"], ["Buffer", "Međuspremnik"],
    ["Start", "Pokreni"], ["Stop", "Zaustavi"], ["Load", "Učitaj"], ["Delete", "Obriši"], ["Delay", "Kašnjenje"], ["Time", "Vrijeme"], ["Pitch", "Visina"], ["Left", "Lijevo"], ["Right", "Desno"],
    ["Mode", "Način"], ["Color", "Boja"], ["Gain", "Pojačanje"], ["Drive", "Distorzija"]].forEach(([e, h]) => E2H.set(e, h));
  const byPattern = (s, list) => { for (const [re, rep] of list) if (re.test(s)) return s.replace(re, rep); return null; };

  /** Translate one text into the current language (unknown texts are kept). */
  function t(s) {
    if (s == null) return s;
    const str = String(s), k = str.trim();
    if (!k) return str;
    let out;
    if (lang === "en") out = H2E.get(k) ?? byPattern(k, PATTERNS);
    else out = E2H.get(k) ?? byPattern(k, EN_PATTERNS);
    if (out == null) return str;
    return str.replace(k, out);
  }
  /** Template with {0}, {1}… placeholders, translated first. */
  function tf(tpl, ...a) { return t(tpl).replace(/\{(\d+)\}/g, (_, i) => a[+i]); }

  // ---------------------------------------------------------------- DOM translation
  const ATTRS = ["placeholder", "title", "aria-label"];
  const skip = (n) => { const p = n.parentElement; return !p || p.closest("script,style,textarea,pre.prompt,.ai-out .md,[data-noi18n]"); };
  function trNode(n) {
    if (n.nodeType === 3) {
      if (skip(n)) return;
      const v = n.nodeValue; if (!v || !v.trim()) return;
      const w = t(v); if (w !== v) n.nodeValue = w;
    } else if (n.nodeType === 1) {
      if (n.closest("script,style,[data-noi18n]")) return;
      for (const a of ATTRS) { const v = n.getAttribute(a); if (v) { const w = t(v); if (w !== v) n.setAttribute(a, w); } }
      if (n.tagName === "TEXTAREA" || (n.tagName === "INPUT" && n.type !== "text" && n.type !== "search" && n.type !== "number" && n.type !== "date")) return;
      const tw = document.createTreeWalker(n, NodeFilter.SHOW_TEXT | NodeFilter.SHOW_ELEMENT);
      let c = tw.nextNode();
      while (c) {
        if (c.nodeType === 3) trNode(c);
        else for (const a of ATTRS) { const v = c.getAttribute(a); if (v) { const w = t(v); if (w !== v) c.setAttribute(a, w); } }
        c = tw.nextNode();
      }
    }
  }
  function start() {
    document.documentElement.lang = lang;
    trNode(document.body);
    new MutationObserver((ms) => {
      for (const m of ms) {
        if (m.type === "characterData") trNode(m.target);
        else if (m.type === "attributes") { const v = m.target.getAttribute(m.attributeName); if (v) { const w = t(v); if (w !== v) m.target.setAttribute(m.attributeName, w); } }
        else m.addedNodes.forEach(trNode);
      }
    }).observe(document.body, { childList: true, subtree: true, characterData: true, attributes: true, attributeFilter: ATTRS });
    // dialogs raised by the scripts
    const c = window.confirm.bind(window), p = window.prompt.bind(window), al = window.alert.bind(window);
    window.confirm = (m) => c(t(m)); window.prompt = (m, d) => p(t(m), d == null ? d : t(d)); window.alert = (m) => al(t(m));
  }
  if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", start); else start();

  // Texts drawn on canvases (axes, legends, readouts) go through the same dictionary.
  const cache = new Map();
  const ft = CanvasRenderingContext2D.prototype.fillText;
  CanvasRenderingContext2D.prototype.fillText = function (s, ...a) {
    if (typeof s === "string" && /[A-Za-zčćžšđČĆŽŠĐ]{2}/.test(s)) {
      let w = cache.get(s);
      if (w === undefined) { w = t(s); if (cache.size > 4000) cache.clear(); cache.set(s, w); }
      s = w;
    }
    return ft.call(this, s, ...a);
  };

  function set(l) { try { localStorage.setItem("ssb.lang", l); } catch (_) {} location.reload(); }
  return {
    get lang() { return lang; }, t, tf, set, apply: (root) => { if (lang === "en") trNode(root); },
    locale: lang === "en" ? "en-GB" : "hr-HR",
    dec: (s) => (lang === "en" ? s : s.replace(".", ",")),
  };
})();
const t = I18N.t;
