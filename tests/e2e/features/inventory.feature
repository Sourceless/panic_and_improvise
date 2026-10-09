Feature: Weapon slots, ammunition and the inventory

  Background:
    Given I am in test room 1
    And I have the smg

  Scenario: The first slot has the Sterling and the second the Hi-Power, and the keys 1 and 2 take them up
    Then the gun in hand is the Sterling L2A3
    When I press 2
    Then the gun in hand is the Browning Hi-Power
    And the magazine has 13 rounds
    When I press 1
    Then the gun in hand is the Sterling L2A3
    And the magazine has 30 rounds

  Scenario: A reload takes its rounds from what is carried
    Given I fire 3 single shots
    And I have 150 spare rounds
    When I press R
    Then the gun is reloading
    And the gun is back up
    And the magazine has 30 rounds
    And I have 147 spare rounds

  Scenario: With no spare rounds a reload does not start
    Given I have no spare rounds
    And I fire 3 single shots
    When I press R
    Then the gun is not reloading
    And the magazine has 27 rounds

  Scenario: A pistol fires one round to a pull, however long the trigger is held
    When I press 2
    And I hold fire for 1 seconds
    Then the gun fired between 1 and 1 shots

  Scenario: The Sterling can be set to fire single shots
    When I press V
    Then the trigger is set to semi
    When I hold fire for 1 seconds
    Then the gun fired between 1 and 1 shots

  Scenario: A different load is chosen with B and goes in at the next reload
    Given I fire 3 single shots
    When I press B
    Then the next reload puts in 9mm hollow point
    When I press R
    And the gun is back up
    Then the magazine has 30 rounds
    And the gun is loaded with 9mm hollow point

  Scenario: Tab opens the inventory, with the mouse free, and Tab closes it
    When I press Tab
    Then the inventory is open and the mouse is free
    When I press Tab
    Then the inventory is closed
